#[macro_use]
extern crate vst;
extern crate time;

use std::f32::consts::PI;
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

/// Simple Gain Effect.
/// Note that this does not use a proper scale for sound and shouldn't be used in
/// a production amplification effect!  This is purely for demonstration purposes,
/// as well as to keep things simple as this is meant to be a starting point for
/// any effect.
struct GainEffect {
    // Store a handle to the plugin's parameter object.
    params: Arc<GainEffectParameters>,
    sample_rate: f32,
    prev_env: f32,
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
    threshold: AtomicFloat,
    ratio: AtomicFloat,
    attack: AtomicFloat,
    release: AtomicFloat,
    gain: AtomicFloat,
}

// All plugins using the `vst` crate will either need to implement the `Default`
// trait, or derive from it.  By implementing the trait, we can set a default value.
// Note that controls will always return a value from 0 - 1.  Setting a default to
// 0.5 means it's halfway up.
impl Default for GainEffect {
    fn default() -> GainEffect {
        GainEffect {
            params: Arc::new(GainEffectParameters::default()),
            sample_rate: 44100.0,
            prev_env: 0.0,
        }
    }
}

impl Default for GainEffectParameters {
    fn default() -> GainEffectParameters {
        GainEffectParameters {
            threshold: AtomicFloat::new(-20.0 / -100.0),
            ratio: AtomicFloat::new(4.0 / 10.0),
            attack: AtomicFloat::new(1.0 / 100.0),
            release: AtomicFloat::new(100.0 / 100.0),
            gain: AtomicFloat::new(1.0 / 100.0),
        }
    }
}

// All plugins using `vst` also need to implement the `Plugin` trait.  Here, we
// define functions that give necessary info to our host.
impl Plugin for GainEffect {
    fn get_info(&self) -> Info {
        Info {
            name: "Compressor".to_string(),
            vendor: "DGriffin".to_string(),
            unique_id: 543923072,
            version: 1,
            inputs: 2,
            outputs: 2,
            // This `parameters` bit is important; without it, none of our
            // parameters will be shown!
            parameters: 5,
            category: Category::Effect,
            ..Default::default()
        }
    }

    fn set_sample_rate(&mut self, rate: f32) {
        self.sample_rate = f32::from(rate);
    }

    // Here is where the bulk of our audio processing code goes.
    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        // Read the amplitude from the parameter object
        let threshold = self.params.threshold.get() * -100.0;
        let ratio = self.params.ratio.get() * 10.0;
        let attack = self.params.attack.get() * 100.0;
        let release = self.params.release.get() * 100.0;
        let gain = gain_from_db(self.params.gain.get() * 100.0);

        let thrlin = gain_from_db(threshold);
        let cte_attack = (-2.0 * PI * 1000.0 / attack / self.sample_rate).exp();
        let cte_release = (-2.0 * PI * 1000.0 / release / self.sample_rate).exp();

        let (inputs, mut outputs) = buffer.split();
        let (inputs_left, inputs_right) = inputs.split_at(1);
        let (mut outputs_left, mut outputs_right) = outputs.split_at_mut(1);

        let inputs_stereo = inputs_left[0].iter().zip(inputs_right[0].iter());
        let outputs_stereo = outputs_left[0].iter_mut().zip(outputs_right[0].iter_mut());

        for (input_pair, output_pair) in inputs_stereo.zip(outputs_stereo) {
            let (input_l, input_r) = input_pair;
            let (output_l, output_r) = output_pair;

            let detector_input = (input_l + input_r).abs() * 0.5;

            // Ballistics filter and envelope generation
            let cte = if detector_input >= self.prev_env {
                cte_attack
            } else {
                cte_release
            };
            let env = detector_input + cte * (self.prev_env - detector_input);
            self.prev_env = env;

            // Compressor transfer function
            let cv = if env <= thrlin {
                1.0
            } else {
                (env / thrlin).powf(1.0 / ratio - 1.0)
            };

            *output_l = *input_l * cv * gain;
            *output_r = *input_r * cv * gain;
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
            0 => self.threshold.get(),
            1 => self.ratio.get(),
            2 => self.attack.get(),
            3 => self.release.get(),
            4 => self.gain.get(),
            _ => 0.0,
        }
    }

    // the `set_parameter` function sets the value of a parameter.
    fn set_parameter(&self, index: i32, val: f32) {
        #[allow(clippy::single_match)]
        match index {
            0 => self.threshold.set(val),
            1 => self.ratio.set(val),
            2 => self.attack.set(val),
            3 => self.release.set(val),
            4 => self.gain.set(val),
            _ => (),
        }
    }

    // This is what will display underneath our control.  We can
    // format it into a string that makes the most since.

    fn get_parameter_text(&self, index: i32) -> String {
        match index {
            0 => format!("{:.2}", self.threshold.get() * -100.0),
            1 => format!("{:.2}", self.ratio.get() * 10.0),
            2 => format!("{:.2}", self.attack.get() * 100.0),
            3 => format!("{:.2}", self.release.get() * 100.0),
            4 => format!("{:.2}", self.gain.get() * 100.0),
            _ => "".to_string(),
        }
    }

    // This shows the control's name.
    fn get_parameter_name(&self, index: i32) -> String {
        match index {
            0 => "Threshold",
            1 => "Ratio",
            2 => "Attack",
            3 => "Release",
            4 => "Gain",
            _ => "",
        }
        .to_string()
    }
}

// This part is important!  Without it, our plugin won't work.
plugin_main!(GainEffect);
