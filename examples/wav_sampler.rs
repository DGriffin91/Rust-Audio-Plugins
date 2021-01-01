// author: doomy <alexander@resamplr.com>

#[macro_use]
extern crate vst;
extern crate dasp;
extern crate dirs;
extern crate dsp_util;
extern crate find_folder;
extern crate hound;
extern crate log;
extern crate log_panics;
extern crate ringbuf;
extern crate simplelog;
extern crate time;

use vst::api::{Events, Supported};
use vst::buffer::AudioBuffer;
use vst::event::Event;
use vst::plugin::{CanDo, Category, Info, Plugin, PluginParameters};
use vst::util::AtomicFloat;

use std::{
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use ringbuf::{Consumer, Producer, RingBuffer};

use dasp::{interpolate::sinc::Sinc, ring_buffer, signal, Signal};
use dasp::{signal::interpolate::Converter, signal::GenMut, slice::ToFrameSliceMut};

use std::thread;

use std::env;

fn setup_logging(path: &str) {
    let log_folder = ::dirs::home_dir().unwrap().join("tmp");

    let _ = ::std::fs::create_dir(log_folder.clone());

    let log_file = ::std::fs::File::create(log_folder.join(path)).unwrap();

    let log_config = ::simplelog::ConfigBuilder::new()
        .set_time_to_local(true)
        .build();

    let _ = ::simplelog::WriteLogger::init(simplelog::LevelFilter::Info, log_config, log_file);

    ::log_panics::init();

    ::log::info!("init");
}

#[derive(Debug, Clone)]
struct WavData {
    audio: Vec<f32>,
    note: usize,
}

fn load_wav(path: &str) -> Vec<f32> {
    // Find and load the wav.
    //let assets = find_folder::Search::ParentsThenKids(5, 5)
    //    .for_folder("assets")
    //    .unwrap();
    //let reader = hound::WavReader::open(assets.join(path)).unwrap();
    let reader = hound::WavReader::open(path).unwrap();
    //let spec = reader.spec();

    // Read the interleaved samples and convert them to a signal.
    let samples = reader.into_samples::<i16>();

    let filter_map = samples.filter_map(Result::ok);

    let frames = signal::from_iter(filter_map);

    let mut output = Vec::new();
    for frame in frames.until_exhausted() {
        output.push(dasp::sample::conv::i16::to_f32(frame));
    }
    output
}

const POLY: usize = 3;
const BASE_SAMPLE_RATE: i32 = 44100;
const SINC_INTERPOLATOR_SIZE: usize = 24;

struct RingBufferSignal {
    consumer: Consumer<f32>,
}

impl RingBufferSignal {
    fn new(capacity: usize) -> (RingBufferSignal, Producer<f32>) {
        let ring = RingBuffer::<f32>::new(capacity);
        let (producer, consumer) = ring.split();
        (RingBufferSignal { consumer }, producer)
    }
}

impl Signal for RingBufferSignal {
    type Frame = f32;

    fn next(&mut self) -> Self::Frame {
        self.consumer.pop().unwrap_or(0.0)
    }
}

/// Simple Gain Effect.
/// Note that this does not use a proper scale for sound and shouldn't be used in
/// a production amplification effect!  This is purely for demonstration purposes,
/// as well as to keep things simple as this is meant to be a starting point for
/// any effect.
struct SamplerSynth {
    // Store a handle to the plugin's parameter object.
    params: Arc<SamplerSynthParameters>,
    audio_data: Vec<Vec<f32>>,
    audio_data_consumer: Option<Consumer<WavData>>,

    sample_rate: f64,
    time: f64,
    notes: [[Note; 64]; POLY],
    current_samples: Vec<f32>,
    current_samples_out: Vec<f32>,
    source_signal: Option<Converter<RingBufferSignal, Sinc<[f32; SINC_INTERPOLATOR_SIZE]>>>,
    source_producer: Option<Producer<f32>>,
    from_size: usize,
    block_size: usize,
    time_per_sample: f64,
}

/// The plugin's parameter object contains the values of parameters that can be
/// adjusted from the host.  If we were creating an effect that didn't allow the
/// user to modify it at runtime or have any controls, we could omit this part.
///
/// The parameters object is shared between the processing and GUI threads.
/// For this reason, all mutable state in the object has to be represented
/// through thread-safe interior mutability. The easiest way to achieve this
/// is to store the parameters in atomic containers.
struct SamplerSynthParameters {
    // The plugin's state consists of a single parameter: amplitude.
    amplitude: AtomicFloat,
}

// All plugins using the `vst` crate will either need to implement the `Default`
// trait, or derive from it.  By implementing the trait, we can set a default value.
// Note that controls will always return a value from 0 - 1.  Setting a default to
// 0.5 means it's halfway up.
impl Default for SamplerSynth {
    fn default() -> SamplerSynth {
        SamplerSynth {
            params: Arc::new(SamplerSynthParameters::default()),
            audio_data: vec![Vec::new(); 64],
            audio_data_consumer: None,
            sample_rate: 44100.0,
            time: 0.0,
            notes: [[Note::default(); 64]; POLY],
            current_samples: Vec::new(),
            current_samples_out: Vec::new(),
            source_signal: None,
            source_producer: None,
            from_size: 64,
            block_size: 64,
            time_per_sample: 44100.0 / 1.0,
        }
    }
}

impl Default for SamplerSynthParameters {
    fn default() -> SamplerSynthParameters {
        SamplerSynthParameters {
            amplitude: AtomicFloat::new(0.5),
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
enum NoteState {
    ON,
    OFF,
    NONE,
}
#[derive(Copy, Clone)]
struct Note {
    sample: usize,
    time: f64,
    off_time: f64,
    level: f32,
    state: NoteState,
}

impl Default for Note {
    fn default() -> Note {
        Note {
            sample: 0,
            time: 0.0,
            off_time: 0.0,
            level: 0.0,
            state: NoteState::NONE,
        }
    }
}

impl SamplerSynth {
    /// Process an incoming midi event.
    ///
    /// The midi data is split up like so:
    ///
    /// `data[0]`: Contains the status and the channel. Source: [source]
    /// `data[1]`: Contains the supplemental data for the message - so, if this was a NoteOn then
    ///            this would contain the note.
    /// `data[2]`: Further supplemental data. Would be velocity in the case of a NoteOn message.
    ///
    /// [source]: http://www.midimountain.com/midi/midi_status.htm
    fn process_midi_event(&mut self, data: [u8; 3]) {
        match data[0] {
            128 => self.note_off(data[1]),
            144 => self.note_on(data[1], data[2]),
            _ => (),
        }
    }

    fn note_on(&mut self, note: u8, level: u8) {
        let note = note as usize;
        for plevel in 0..POLY {
            if self.notes[plevel][note].state == NoteState::NONE {
                self.notes[plevel][note] = Note {
                    sample: 0,
                    time: 0.0,
                    off_time: 0.0,
                    level: (level as f32) / 255.0,
                    state: NoteState::ON,
                };
                return;
            }
        }
    }

    fn note_off(&mut self, note: u8) {
        let note = note as usize;
        //Just picking which is on and setting it to off may not work
        for plevel in 0..POLY {
            if self.notes[plevel][note].state == NoteState::ON {
                self.notes[plevel][note].state = NoteState::OFF;
            }
        }
    }

    fn process_sample(&mut self, amplitude: f32, sample_idx: usize) {
        let mut output_sample = 0.0;
        for plevel in 0..POLY {
            for note_value in 0..64usize {
                let note = &mut self.notes[plevel][note_value];
                match note.state {
                    NoteState::ON | NoteState::OFF => {
                        if note_value == 1 {
                            output_sample = 1.0;
                            note.state = NoteState::OFF;
                        }

                        //We need to play the sound all the way through, even if it's off
                        if note.sample >= self.audio_data[note_value].len() {
                            *note = Note::default();
                            continue;
                        }

                        output_sample += self.audio_data[note_value][note.sample] * note.level;

                        note.time += self.time_per_sample;
                        note.sample += 1;
                    }
                    NoteState::NONE => {}
                }
            }
        }

        if self.sample_rate as i32 != BASE_SAMPLE_RATE {
            match &mut self.source_producer {
                Some(s) => Ok(s.push(output_sample * amplitude)),
                None => Err("Could not access producer"),
            };
        } else {
            self.current_samples[sample_idx] = output_sample * amplitude;
        }

        self.time += self.time_per_sample;
    }
}

fn start_file_load_thread(mut producer: Producer<WavData>) {
    //Start up a thread to load the wav files form disk
    thread::spawn(move || {
        ::log::info!("init thread");
        producer
            .push(WavData {
                audio: load_wav("C:/dev/vst/dgriffin/assets/kick.wav"),
                note: 36,
            })
            .unwrap();
        producer
            .push(WavData {
                audio: load_wav("C:/dev/vst/dgriffin/assets/snare.wav"),
                note: 38,
            })
            .unwrap();
        producer
            .push(WavData {
                audio: load_wav("C:/dev/vst/dgriffin/assets/floor.wav"),
                note: 41,
            })
            .unwrap();
        producer
            .push(WavData {
                audio: load_wav("C:/dev/vst/dgriffin/assets/rack.wav"),
                note: 43,
            })
            .unwrap();
        producer
            .push(WavData {
                audio: load_wav("C:/dev/vst/dgriffin/assets/sweep.wav"),
                note: 2,
            })
            .unwrap();

        ::log::info!("init thread done loading");
    });
}

// All plugins using `vst` also need to implement the `Plugin` trait.  Here, we
// define functions that give necessary info to our host.
impl Plugin for SamplerSynth {
    fn get_info(&self) -> Info {
        Info {
            name: "Wav Sampler in Rust".to_string(),
            vendor: "DGriffin".to_string(),
            unique_id: 241723055,
            version: 1,
            inputs: 2,
            outputs: 2,
            // This `parameters` bit is important; without it, none of our
            // parameters will be shown!
            parameters: 1,
            category: Category::Synth,
            ..Default::default()
        }
    }

    fn init(&mut self) {
        setup_logging("WAVSampler.log");

        //let path = env::current_dir().unwrap();
        //::log::info!("The current directory is {}", path.display());
        //::log::info!("std::env::current_exe() {:?}", std::env::current_exe());
    }

    // Here is where the bulk of our audio processing code goes.
    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        if let Some(ref mut consumer) = self.audio_data_consumer {
            for _ in 0..consumer.len() {
                if let Some(wav_data) = consumer.pop() {
                    self.audio_data[wav_data.note] = wav_data.audio;
                } else {
                    break;
                }
            }
        } else {
            let audio_data_ring = RingBuffer::<WavData>::new(64);

            let (audio_data_producer, audio_data_consumer) = audio_data_ring.split();
            self.audio_data_consumer = Some(audio_data_consumer);

            start_file_load_thread(audio_data_producer);
        }

        // Read the amplitude from the parameter object
        let amplitude = self.params.amplitude.get();
        // First, we destructure our audio buffer into an arbitrary number of
        // input and output buffers.  Usually, we'll be dealing with stereo (2 of each)
        // but that might change.
        let samples = buffer.samples();
        let (_, mut outputs) = buffer.split();
        let output_count = outputs.len();
        let mut count = self.from_size;
        if self.sample_rate as i32 != BASE_SAMPLE_RATE {
            count = (count as f64 * 1.1) as usize;
        }

        //let mut n = 0;
        if self.sample_rate as i32 != BASE_SAMPLE_RATE {
            for sample_idx in 0..count {
                match &mut self.source_producer {
                    Some(s) => Ok({
                        if s.is_full() {
                            break;
                        }
                    }),
                    None => Err("Could not access source_signal"),
                };
                self.process_sample(amplitude, sample_idx);
            }

            for i in 0..samples {
                match &mut self.source_signal {
                    Some(s) => Ok(self.current_samples_out[i] = s.next()),
                    None => Err("Could not access producer"),
                };
            }
        } else {
            for sample_idx in 0..count {
                self.process_sample(amplitude, sample_idx)
            }
        }

        if self.sample_rate as i32 != BASE_SAMPLE_RATE {
            for i in 0..samples {
                for buf_idx in 0..output_count {
                    let buff = outputs.get_mut(buf_idx);
                    buff[i] = self.current_samples_out[i];
                }
            }
        } else {
            for i in 0..samples {
                for buf_idx in 0..output_count {
                    let buff = outputs.get_mut(buf_idx);
                    buff[i] = self.current_samples[i];
                }
            }
        }
    }

    #[allow(unused_variables)]
    #[allow(clippy::single_match)]
    fn process_events(&mut self, events: &Events) {
        for event in events.events() {
            match event {
                Event::Midi(ev) => self.process_midi_event(ev.data),
                // More events can be handled here.
                _ => (),
            }
        }
    }

    // Return the parameter object. This method can be omitted if the
    // plugin has no parameters.
    fn get_parameter_object(&mut self) -> Arc<dyn PluginParameters> {
        Arc::clone(&self.params) as Arc<dyn PluginParameters>
    }

    fn can_do(&self, can_do: CanDo) -> Supported {
        match can_do {
            CanDo::ReceiveMidiEvent => Supported::Yes,
            _ => Supported::Maybe,
        }
    }

    fn set_sample_rate(&mut self, rate: f32) {
        self.sample_rate = rate as f64;
        self.time_per_sample = (1.0 / self.sample_rate) as f64;
    }

    fn set_block_size(&mut self, size: i64) {
        let sinc = Sinc::new(ring_buffer::Fixed::from([0.0f32; SINC_INTERPOLATOR_SIZE]));

        self.from_size =
            (size as f32 * (BASE_SAMPLE_RATE as f32 / self.sample_rate as f32)) as usize;
        self.block_size = size as usize;

        self.current_samples = vec![0.0; self.from_size as usize];
        self.current_samples_out = vec![0.0; size as usize];

        let (source_signal, producer) = RingBufferSignal::new((self.from_size + 1) as usize);

        let source_signal =
            source_signal.from_hz_to_hz(sinc, BASE_SAMPLE_RATE as f64, self.sample_rate as f64);

        self.source_signal = Some(source_signal);

        self.source_producer = Some(producer);
    }
}

impl PluginParameters for SamplerSynthParameters {
    // the `get_parameter` function reads the value of a parameter.
    fn get_parameter(&self, index: i32) -> f32 {
        match index {
            0 => self.amplitude.get(),
            _ => 0.0,
        }
    }

    // the `set_parameter` function sets the value of a parameter.
    fn set_parameter(&self, index: i32, val: f32) {
        #[allow(clippy::single_match)]
        match index {
            0 => self.amplitude.set(val),
            _ => (),
        }
    }

    // This is what will display underneath our control.  We can
    // format it into a string that makes the most since.
    fn get_parameter_text(&self, index: i32) -> String {
        match index {
            0 => format!("{:.2}", (self.amplitude.get() - 0.5) * 2f32),
            _ => "".to_string(),
        }
    }

    // This shows the control's name.
    fn get_parameter_name(&self, index: i32) -> String {
        match index {
            0 => "Amplitude",
            _ => "",
        }
        .to_string()
    }
}

// This part is important!  Without it, our plugin won't work.
plugin_main!(SamplerSynth);
