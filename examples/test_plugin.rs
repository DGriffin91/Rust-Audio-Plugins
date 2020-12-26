// author: doomy <alexander@resamplr.com>

#[macro_use]
extern crate vst;
extern crate time;

use vst::buffer::AudioBuffer;
use vst::plugin::{Category, Info, Plugin, PluginParameters};
use vst::util::AtomicFloat;

use std::sync::Arc;

fn gain_from_db(decibels: f32) -> f32 {
    (10.0f32).powf(decibels * 0.05)
}

fn db_from_gain(gain: f32) -> f32 {
    gain.max(0.0).log(10.0) * 20.0
}

fn to_range(x: f32, bottom: f32, top: f32) -> f32 {
    x * (top - bottom) + bottom
}

fn from_range(x: f32, bottom: f32, top: f32) -> f32 {
    (x - bottom) / (top - bottom)
}

/// Simple Gain Effect.
/// Note that this does not use a proper scale for sound and shouldn't be used in
/// a production amplification effect!  This is purely for demonstration purposes,
/// as well as to keep things simple as this is meant to be a starting point for
/// any effect.
struct ReverbEffect {
    // Store a handle to the plugin's parameter object.
    params: Arc<ReverbEffectParameters>,
    sample_rate: f32,
}

// All plugins using `vst` also need to implement the `Plugin` trait.  Here, we
// define functions that give necessary info to our host.
impl Plugin for ReverbEffect {
    fn get_info(&self) -> Info {
        Info {
            name: "Test Plugin".to_string(),
            vendor: "DGriffin".to_string(),
            unique_id: 243723012,
            version: 1,
            inputs: 2,
            outputs: 2,
            // This `parameters` bit is important; without it, none of our
            // parameters will be shown!
            parameters: 11,
            category: Category::Effect,
            ..Default::default()
        }
    }

    fn set_sample_rate(&mut self, rate: f32) {
        self.sample_rate = f32::from(rate);
    }

    // Here is where the bulk of our audio processing code goes.
    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        let reverb_master = self.params.reverb_master.get();

        let (inputs, mut outputs) = buffer.split();
        let (inputs_left, inputs_right) = inputs.split_at(1);
        let (mut outputs_left, mut outputs_right) = outputs.split_at_mut(1);

        let inputs_stereo = inputs_left[0].iter().zip(inputs_right[0].iter());
        let outputs_stereo = outputs_left[0].iter_mut().zip(outputs_right[0].iter_mut());

        for (input_pair, output_pair) in inputs_stereo.zip(outputs_stereo) {
            let (input_l, input_r) = input_pair;
            let (output_l, output_r) = output_pair;

            *output_l = *input_l * reverb_master;
            *output_r = *input_r * reverb_master;
        }
    }

    // Return the parameter object. This method can be omitted if the
    // plugin has no parameters.
    fn get_parameter_object(&mut self) -> Arc<dyn PluginParameters> {
        Arc::clone(&self.params) as Arc<dyn PluginParameters>
    }
}

/// The plugin's parameter object contains the values of parameters that can be
/// adjusted from the host.  If we were creating an effect that didn't allow the
/// user to modify it at runtime or have any controls, we could omit this part.
///
/// The parameters object is shared between the processing and GUI threads.
/// For this reason, all mutable state in the object has to be represented
/// through thread-safe interior mutability. The easiest way to achieve this
/// is to store the parameters in atomic containers.
struct ReverbEffectParameters {
    // The plugin's state consists of a single parameter: amplitude.
    mix: AtomicFloat,
    delay_size: AtomicFloat,
    delay_delta: AtomicFloat,
    decay_init: AtomicFloat,
    decay_delta: AtomicFloat,
    iterations: AtomicFloat,
    lpf_cutoff: AtomicFloat,
    lpf_slope: AtomicFloat,
    saturation_mix: AtomicFloat,
    saturation: AtomicFloat,
    reverb_master: AtomicFloat,
}

// All plugins using the `vst` crate will either need to implement the `Default`
// trait, or derive from it.  By implementing the trait, we can set a default value.
// Note that controls will always return a value from 0 - 1.  Setting a default to
// 0.5 means it's halfway up.
impl Default for ReverbEffect {
    fn default() -> ReverbEffect {
        ReverbEffect {
            params: Arc::new(ReverbEffectParameters::default()),
            sample_rate: 44100.0,
        }
    }
}

impl Default for ReverbEffectParameters {
    fn default() -> ReverbEffectParameters {
        ReverbEffectParameters {
            mix: AtomicFloat::new(0.5),
            delay_size: AtomicFloat::new(0.2),
            delay_delta: AtomicFloat::new(0.9),
            decay_init: AtomicFloat::new(0.9),
            decay_delta: AtomicFloat::new(1.0),
            iterations: AtomicFloat::new(16.0),
            lpf_cutoff: AtomicFloat::new(20000.0),
            lpf_slope: AtomicFloat::new(0.2),
            saturation_mix: AtomicFloat::new(0.0),
            saturation: AtomicFloat::new(1.0),
            reverb_master: AtomicFloat::new(gain_from_db(0.0)),
        }
    }
}

impl PluginParameters for ReverbEffectParameters {
    // the `get_parameter` function reads the value of a parameter.
    fn get_parameter(&self, index: i32) -> f32 {
        match index {
            0 => self.mix.get(),
            1 => self.delay_size.get(),
            2 => from_range(self.delay_delta.get(), 0.6, 1.5),
            3 => from_range(self.decay_init.get(), 0.0, 1.5),
            4 => from_range(self.decay_delta.get(), 0.5, 1.5),
            5 => from_range(self.iterations.get(), 1.0, 64.0).floor(),
            6 => from_range(self.lpf_cutoff.get(), 1.0, 20000.0),
            7 => from_range(self.lpf_slope.get(), 0.04, 1.0),
            8 => self.saturation_mix.get(),
            9 => from_range(self.saturation.get(), 0.0, 100.0),
            10 => from_range(db_from_gain(self.reverb_master.get()), -24.0, 24.0),
            _ => 0.0,
        }
    }

    // the `set_parameter` function sets the value of a parameter.
    fn set_parameter(&self, index: i32, val: f32) {
        #[allow(clippy::single_match)]
        match index {
            0 => self.mix.set(val),
            1 => self.delay_size.set(val),
            2 => self.delay_delta.set(to_range(val, 0.6, 1.5)),
            3 => self.decay_init.set(to_range(val, 0.0, 1.5)),
            4 => self.decay_delta.set(to_range(val, 0.5, 1.5)),
            5 => self.iterations.set(to_range(val, 1.0, 64.0)),
            6 => self.lpf_cutoff.set(to_range(val, 1.0, 20000.0)),
            7 => self.lpf_slope.set(to_range(val, 0.04, 1.0)),
            8 => self.saturation_mix.set(val),
            9 => self.saturation.set(to_range(val, 0.0, 100.0)),
            10 => self
                .reverb_master
                .set(gain_from_db(to_range(val, -24.0, 24.0))),
            _ => (),
        }
    }

    // This is what will display underneath our control.  We can
    // format it into a string that makes the most since.
    fn get_parameter_text(&self, index: i32) -> String {
        match index {
            0 => format!("{:.2}", self.mix.get()),
            1 => format!("{:.2}", self.delay_size.get()),
            2 => format!("{:.2}", self.delay_delta.get()),
            3 => format!("{:.2}", self.decay_init.get()),
            4 => format!("{:.2}", self.decay_delta.get()),
            5 => format!("{:.2}", self.iterations.get()),
            6 => format!("{:.2}", self.lpf_cutoff.get()),
            7 => format!("{:.2}", self.lpf_slope.get()),
            8 => format!("{:.2}", self.saturation_mix.get()),
            9 => format!("{:.2}", self.saturation.get()),
            10 => format!("{:.2}", db_from_gain(self.reverb_master.get())),

            _ => "".to_string(),
        }
    }

    // This shows the control's name.
    fn get_parameter_name(&self, index: i32) -> String {
        match index {
            0 => "Mix",
            1 => "Delay size",
            2 => "Delay delta",
            3 => "Decay init",
            4 => "Decay delta",
            5 => "Iterations",
            6 => "LPF cutoff",
            7 => "LPF slope",
            8 => "Saturation mix",
            9 => "Saturation",
            10 => "Reverb master",
            _ => "",
        }
        .to_string()
    }
}

// This part is important!  Without it, our plugin won't work.
plugin_main!(ReverbEffect);
