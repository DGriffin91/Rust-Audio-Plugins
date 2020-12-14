#[macro_use]
extern crate vst;
extern crate time;

use vst::buffer::AudioBuffer;
use vst::plugin::{Category, Info, Plugin, PluginParameters};
use vst::util::AtomicFloat;

use std::sync::Arc;

/// Simple Gain Effect.
/// Note that this does not use a proper scale for sound and shouldn't be used in
/// a production amplification effect!  This is purely for demonstration purposes,
/// as well as to keep things simple as this is meant to be a starting point for
/// any effect.
struct GainEffect {
    // Store a handle to the plugin's parameter object.
    params: Arc<GainEffectParameters>,
    sample_rate: f32,
    prev_l: f32,
    prev_r: f32,
}

/// The plugin's parameter object contains the values of parameters that can be
/// adjusted from the host.  If we were creating an effect that didn't allow the
/// user to modify it at runtime or have any controls, we could omit this part.
///
/// The parameters object is shared between the processing and GUI threads.
/// For this reason, all mutable state in the object has to be represented
/// through thread-safe interior mutability. The easiest way to achieve this
/// is to store the parameters in atomic containers.
struct GainEffectParameters {
    // The plugin's state consists of a single parameter: amplitude.
    slew_min: AtomicFloat,
    slew_max: AtomicFloat,
    rise: AtomicFloat,
    fall: AtomicFloat,
}

// All plugins using the `vst` crate will either need to implement the `Default`
// trait, or derive from it.  By implementing the trait, we can set a default value.
// Note that controls will always return a value from 0 - 1.  Setting a default to
// 0.5 means it's halfway up.
impl Default for GainEffect {
    fn default() -> GainEffect {
        GainEffect {
            params: Arc::new(GainEffectParameters::default()),
            prev_l: 0.0,
            prev_r: 0.0,
            sample_rate: 44100.0,
        }
    }
}

impl Default for GainEffectParameters {
    fn default() -> GainEffectParameters {
        GainEffectParameters {
            slew_min: AtomicFloat::new(0.1),
            slew_max: AtomicFloat::new(10000.0 / 100000.0),
            rise: AtomicFloat::new(0.5),
            fall: AtomicFloat::new(0.5),
        }
    }
}

fn mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

// All plugins using `vst` also need to implement the `Plugin` trait.  Here, we
// define functions that give necessary info to our host.
impl Plugin for GainEffect {
    fn get_info(&self) -> Info {
        Info {
            name: "Slew".to_string(),
            vendor: "DGriffin".to_string(),
            unique_id: 435670317,
            version: 1,
            inputs: 2,
            outputs: 2,
            // This `parameters` bit is important; without it, none of our
            // parameters will be shown!
            parameters: 4,
            category: Category::Effect,
            ..Default::default()
        }
    }

    // Here is where the bulk of our audio processing code goes.
    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        let time_step = 1.0 / self.sample_rate;

        let slew_min = self.params.slew_min.get();
        let slew_max = self.params.slew_max.get() * 100000.0;
        let rise = self.params.rise.get();
        let fall = self.params.fall.get();

        let slew_rise = slew_max * time_step * (slew_min / slew_max).powf(rise);
        let slew_fall = slew_max * time_step * (slew_min / slew_max).powf(fall);

        // First, we destructure our audio buffer into an arbitrary number of
        // input and output buffers.  Usually, we'll be dealing with stereo (2 of each)
        // but that might change.

        let (inputs, mut outputs) = buffer.split();
        let (inputs_left, inputs_right) = inputs.split_at(1);
        let (mut outputs_left, mut outputs_right) = outputs.split_at_mut(1);

        let inputs_stereo = inputs_left[0].iter().zip(inputs_right[0].iter());
        let outputs_stereo = outputs_left[0].iter_mut().zip(outputs_right[0].iter_mut());

        for (input_pair, output_pair) in inputs_stereo.zip(outputs_stereo) {
            let (input_l, input_r) = input_pair;
            let (output_l, output_r) = output_pair;

            *output_l = if *input_l > self.prev_l {
                input_l.min(self.prev_l + slew_rise)
            } else {
                input_l.max(self.prev_l - slew_fall)
            };

            *output_r = if *input_r > self.prev_r {
                input_r.min(self.prev_r + slew_rise)
            } else {
                input_r.max(self.prev_r - slew_fall)
            };

            self.prev_l = *output_l;
            self.prev_r = *output_r;
        }
    }

    // Return the parameter object. This method can be omitted if the
    // plugin has no parameters.
    fn get_parameter_object(&mut self) -> Arc<dyn PluginParameters> {
        Arc::clone(&self.params) as Arc<dyn PluginParameters>
    }
}

impl PluginParameters for GainEffectParameters {
    // the `get_parameter` function reads the value of a parameter.
    fn get_parameter(&self, index: i32) -> f32 {
        match index {
            0 => self.slew_min.get(),
            1 => self.slew_max.get(),
            2 => self.rise.get(),
            3 => self.fall.get(),
            _ => 0.0,
        }
    }

    // the `set_parameter` function sets the value of a parameter.
    fn set_parameter(&self, index: i32, val: f32) {
        #[allow(clippy::single_match)]
        match index {
            0 => self.slew_min.set(val),
            1 => self.slew_max.set(val),
            2 => self.rise.set(val),
            3 => self.fall.set(val),
            _ => (),
        }
    }

    // This is what will display underneath our control.  We can
    // format it into a string that makes the most since.
    fn get_parameter_text(&self, index: i32) -> String {
        match index {
            0 => format!("{:.2}", self.slew_min.get()),
            1 => format!("{:.2}", self.slew_max.get() * 100000.0),
            2 => format!("{:.2}", self.rise.get()),
            3 => format!("{:.2}", self.fall.get()),
            _ => "".to_string(),
        }
    }

    // This shows the control's name.
    fn get_parameter_name(&self, index: i32) -> String {
        match index {
            0 => "Slew Min v/s",
            1 => "Slew Max v/s",
            2 => "Rise",
            3 => "Fall",
            _ => "",
        }
        .to_string()
    }
}

// This part is important!  Without it, our plugin won't work.
plugin_main!(GainEffect);
