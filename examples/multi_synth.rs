#[macro_use]
extern crate vst;

use std::f64::consts::PI;
use std::sync::Arc;
use vst::api::{Events, Supported};
use vst::buffer::AudioBuffer;
use vst::event::Event;
use vst::plugin::{CanDo, Category, Info, Plugin, PluginParameters};
use vst::util::AtomicFloat;

/// Convert the midi note's pitch into the equivalent frequency.
///
/// This function assumes A4 is 440hz.
fn midi_pitch_to_freq(pitch: u8) -> f64 {
    const A4_PITCH: i8 = 69;
    const A4_FREQ: f64 = 440.0;

    // Midi notes can be 0-127
    ((f64::from(pitch as i8 - A4_PITCH)) / 12.).exp2() * A4_FREQ
}

struct SineSynthParameters {
    // The plugin's state consists of a single parameter: amplitude.
    amplitude: AtomicFloat,
    attack: AtomicFloat,
    decay: AtomicFloat,
    sustain: AtomicFloat,
    release: AtomicFloat,
    sine: AtomicFloat,
    triangle: AtomicFloat,
    saw: AtomicFloat,
    square: AtomicFloat,
}

impl Default for SineSynthParameters {
    fn default() -> SineSynthParameters {
        SineSynthParameters {
            amplitude: AtomicFloat::new(0.5),
            attack: AtomicFloat::new(0.5),
            decay: AtomicFloat::new(0.5),
            sustain: AtomicFloat::new(0.5),
            release: AtomicFloat::new(0.5),
            sine: AtomicFloat::new(1.0),
            triangle: AtomicFloat::new(0.0),
            saw: AtomicFloat::new(0.0),
            square: AtomicFloat::new(0.0),
        }
    }
}

impl PluginParameters for SineSynthParameters {
    // the `get_parameter` function reads the value of a parameter.
    fn get_parameter(&self, index: i32) -> f32 {
        match index {
            0 => self.amplitude.get(),
            1 => self.attack.get(),
            3 => self.decay.get(),
            2 => self.sustain.get(),
            4 => self.release.get(),
            5 => self.sine.get(),
            6 => self.triangle.get(),
            7 => self.saw.get(),
            8 => self.square.get(),
            _ => 0.0,
        }
    }

    // the `set_parameter` function sets the value of a parameter.
    fn set_parameter(&self, index: i32, val: f32) {
        #[allow(clippy::single_match)]
        match index {
            0 => self.amplitude.set(val),
            1 => self.attack.set(val),
            2 => self.decay.set(val),
            3 => self.sustain.set(val),
            4 => self.release.set(val),
            5 => self.sine.set(val),
            6 => self.triangle.set(val),
            7 => self.saw.set(val),
            8 => self.square.set(val),
            _ => (),
        }
    }

    // This is what will display underneath our control.  We can
    // format it into a string that makes the most since.
    fn get_parameter_text(&self, index: i32) -> String {
        match index {
            0 => format!("{:.2}", (self.amplitude.get() - 0.5) * 2f32),
            1 => format!("{:.2}", (self.attack.get())),
            2 => format!("{:.2}", (self.decay.get())),
            3 => format!("{:.2}", (self.sustain.get() - 0.5) * 2f32),
            4 => format!("{:.2}", (self.release.get())),
            5 => format!("{:.2}", (self.sine.get())),
            6 => format!("{:.2}", (self.triangle.get())),
            7 => format!("{:.2}", (self.saw.get())),
            8 => format!("{:.2}", (self.square.get())),
            _ => "".to_string(),
        }
    }

    // This shows the control's name.
    fn get_parameter_name(&self, index: i32) -> String {
        match index {
            0 => "Amplitude",
            1 => "Attack",
            2 => "Decay",
            3 => "Sustain",
            4 => "Release",
            5 => "Sine",
            6 => "Triangle",
            7 => "Saw",
            8 => "Square",
            _ => "",
        }
        .to_string()
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
    time: f64,
    off_time: f64,
    level: f64,
    state: NoteState,
}

impl Default for Note {
    fn default() -> Note {
        Note {
            time: 0.0,
            off_time: 0.0,
            level: 0.0,
            state: NoteState::NONE,
        }
    }
}

struct SineSynth {
    sample_rate: f64,
    time: f64,
    notes: [[Note; 256]; 8],
    params: Arc<SineSynthParameters>,
}

impl Default for SineSynth {
    fn default() -> SineSynth {
        SineSynth {
            sample_rate: 44100.0,
            time: 0.0,
            notes: [[Note::default(); 256]; 8],
            params: Arc::new(SineSynthParameters::default()),
        }
    }
}

impl SineSynth {
    fn time_per_sample(&self) -> f64 {
        1.0 / self.sample_rate
    }

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
        for plevel in 0..7 {
            if self.notes[plevel][note].state == NoteState::NONE {
                self.notes[plevel][note] = Note {
                    time: 0.0,
                    off_time: 0.0,
                    level: (level as f64) / 255.0,
                    state: NoteState::ON,
                };
                return;
            }
        }
    }

    fn note_off(&mut self, note: u8) {
        let note = note as usize;
        //Just picking which is on and setting it to off may not work
        for plevel in 0..7 {
            if self.notes[plevel][note].state == NoteState::ON {
                self.notes[plevel][note].state = NoteState::OFF;
            }
        }
    }
}

pub const TAU: f64 = PI * 2.0;

fn mix(x: f64, y: f64, a: f64) -> f64 {
    x * (1.0 - a) + y * a
}

fn triangle(n: f64) -> f64 {
    (saw(n + PI / 2.0)).abs() * 2.0 - 1.0
}

fn saw(n: f64) -> f64 {
    (((n + PI) % TAU) / PI) - 1.0
}

fn square(n: f64) -> f64 {
    (n.sin() * 100.0).max(0.0).min(2.0) - 1.0
}

fn sine_note(t: f64, note_value: u8) -> f64 {
    (t * midi_pitch_to_freq(note_value) * TAU).sin()
}

fn triangle_note(t: f64, note_value: u8) -> f64 {
    triangle(t * midi_pitch_to_freq(note_value) * TAU)
}

fn saw_note(t: f64, note_value: u8) -> f64 {
    saw(t * midi_pitch_to_freq(note_value) * TAU)
}

fn square_note(t: f64, note_value: u8) -> f64 {
    square(t * midi_pitch_to_freq(note_value) * TAU)
}

impl Plugin for SineSynth {
    fn get_info(&self) -> Info {
        Info {
            name: "MultiSynth".to_string(),
            vendor: "DGriffin".to_string(),
            unique_id: 234873245,
            category: Category::Synth,
            inputs: 2,
            outputs: 2,
            parameters: 9,
            initial_delay: 0,
            ..Info::default()
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

    fn set_sample_rate(&mut self, rate: f32) {
        self.sample_rate = f64::from(rate);
    }

    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        let amplitude = self.params.amplitude.get();
        let attack = self.params.attack.get() as f64;
        let decay = self.params.decay.get() as f64;
        let sustain = self.params.sustain.get() as f64;
        let release = self.params.release.get() as f64;

        let sine_level = self.params.sine.get() as f64;
        let triangle_level = self.params.triangle.get() as f64;
        let saw_level = self.params.saw.get() as f64;
        let square_level = self.params.square.get() as f64;

        let samples = buffer.samples();
        let (_, mut outputs) = buffer.split();
        let output_count = outputs.len();
        let per_sample = self.time_per_sample();
        let mut output_sample;
        for sample_idx in 0..samples {
            output_sample = 0.0;
            for plevel in 0..7 {
                for note_value in 0..255 {
                    let note = &mut self.notes[plevel][note_value as usize];
                    let on_alpha = if note.state != NoteState::NONE {
                        if note.time < attack {
                            note.time / attack
                        } else if note.time < attack + decay {
                            mix(1.0, sustain, (note.time - attack) / decay)
                        } else {
                            sustain
                        }
                    } else {
                        0.0
                    };
                    match note.state {
                        NoteState::ON => {
                            let mut signal = 0.0;
                            signal += sine_note(self.time, note_value) * note.level * sine_level;
                            signal +=
                                triangle_note(self.time, note_value) * note.level * triangle_level;
                            signal += saw_note(self.time, note_value) * note.level * saw_level;
                            signal +=
                                square_note(self.time, note_value) * note.level * square_level;

                            output_sample += (signal * on_alpha) as f32;

                            note.time += per_sample;
                        }
                        NoteState::OFF => {
                            let mut signal = 0.0;
                            signal += sine_note(self.time, note_value) * note.level * sine_level;
                            signal +=
                                triangle_note(self.time, note_value) * note.level * triangle_level;
                            signal += saw_note(self.time, note_value) * note.level * saw_level;
                            signal +=
                                square_note(self.time, note_value) * note.level * square_level;

                            if note.off_time < release {
                                let alpha = mix(on_alpha, 0.0, note.off_time / release)
                                    .max(0.0)
                                    .min(1.0);
                                output_sample += (signal * alpha) as f32;

                                note.time += per_sample;
                                note.off_time += per_sample;
                            } else {
                                *note = Note::default();
                            }
                        }
                        NoteState::NONE => {}
                    }
                }
            }

            for buf_idx in 0..output_count {
                let buff = outputs.get_mut(buf_idx);
                buff[sample_idx] = output_sample * amplitude;
            }

            self.time += per_sample;
        }
    }

    fn get_parameter_object(&mut self) -> Arc<dyn PluginParameters> {
        Arc::clone(&self.params) as Arc<dyn PluginParameters>
    }

    fn can_do(&self, can_do: CanDo) -> Supported {
        match can_do {
            CanDo::ReceiveMidiEvent => Supported::Yes,
            _ => Supported::Maybe,
        }
    }
}

plugin_main!(SineSynth);

#[cfg(test)]
mod tests {
    use midi_pitch_to_freq;

    #[test]
    fn test_midi_pitch_to_freq() {
        for i in 0..127 {
            // expect no panics
            midi_pitch_to_freq(i);
        }
    }
}
