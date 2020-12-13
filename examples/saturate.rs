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

    output_prev_l: f32,
    input_prev_l: f32,
    output_prev_r: f32,
    input_prev_r: f32,
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
    gain: AtomicFloat,
    master: AtomicFloat,
    a_gain: AtomicFloat,
    b_gain: AtomicFloat,
    ab_mix: AtomicFloat,
}

// All plugins using the `vst` crate will either need to implement the `Default`
// trait, or derive from it.  By implementing the trait, we can set a default value.
// Note that controls will always return a value from 0 - 1.  Setting a default to
// 0.5 means it's halfway up.
impl Default for GainEffect {
    fn default() -> GainEffect {
        GainEffect {
            params: Arc::new(GainEffectParameters::default()),
            output_prev_l: 0.0,
            input_prev_l: 0.0,
            output_prev_r: 0.0,
            input_prev_r: 0.0,
        }
    }
}

impl Default for GainEffectParameters {
    fn default() -> GainEffectParameters {
        GainEffectParameters {
            gain: AtomicFloat::new(0.0),
            master: AtomicFloat::new(1.0),
            a_gain: AtomicFloat::new(1.0),
            b_gain: AtomicFloat::new(1.0),
            ab_mix: AtomicFloat::new(0.5),
        }
    }
}

fn mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

//let delta_input = input - input_prev;
//(output_prev + a * ((input * 2.0).tanh() - output_prev) * delta_input.abs() + b * delta_input / (input * 2.0).cosh().powi(2)).tanh()

fn saturate(output_prev: f32, input_prev: f32, input: f32, a: f32, b: f32, ab_mix: f32) -> f32 {
    let delta_input = input - input_prev;
    let dist_a = ((a * input).tanh() - output_prev) * a * delta_input.abs();
    let dist_b = b * delta_input / (b * input).cosh().powi(2);
    mix(
        (output_prev + dist_a).tanh(),
        (output_prev + dist_b).tanh() * 12.0,
        ab_mix.max(0.0).min(1.0),
    )
}

// All plugins using `vst` also need to implement the `Plugin` trait.  Here, we
// define functions that give necessary info to our host.
impl Plugin for GainEffect {
    fn get_info(&self) -> Info {
        Info {
            name: "Saturate".to_string(),
            vendor: "DGriffin".to_string(),
            unique_id: 437230317,
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

    // Here is where the bulk of our audio processing code goes.
    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        // Read the amplitude from the parameter object
        let a = self.params.a_gain.get() * 12.0;
        let b = self.params.b_gain.get() * 1.0;
        let ab_mix = self.params.ab_mix.get();
        let gain = (self.params.gain.get() * 100.0) + 1.0;
        let master = 1.0 / ((self.params.master.get() * 100.0) + 1.0);
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

            let l = *input_l * gain;
            let r = *input_r * gain;

            *output_l = saturate(self.output_prev_l, self.input_prev_l, l, a, b, ab_mix);

            self.input_prev_l = l;
            self.output_prev_l = *output_l;

            *output_r = saturate(self.output_prev_r, self.input_prev_r, r, a, b, ab_mix);

            self.input_prev_r = r;
            self.output_prev_r = *output_r;

            *output_l = *output_l * master;
            *output_r = *output_r * master;
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
            0 => self.gain.get(),
            1 => self.master.get(),
            2 => self.a_gain.get(),
            3 => self.b_gain.get(),
            4 => self.ab_mix.get(),
            _ => 0.0,
        }
    }

    // the `set_parameter` function sets the value of a parameter.
    fn set_parameter(&self, index: i32, val: f32) {
        #[allow(clippy::single_match)]
        match index {
            0 => self.gain.set(val),
            1 => self.master.set(val),
            2 => self.a_gain.set(val),
            3 => self.b_gain.set(val),
            4 => self.ab_mix.set(val),
            _ => (),
        }
    }

    // This is what will display underneath our control.  We can
    // format it into a string that makes the most since.
    fn get_parameter_text(&self, index: i32) -> String {
        match index {
            0 => format!("{:.2}", self.gain.get() * 48.0),
            1 => format!("{:.2}", -self.master.get() * 48.0),
            2 => format!("{:.2}", self.a_gain.get()),
            3 => format!("{:.2}", self.b_gain.get()),
            4 => format!("{:.2}", self.ab_mix.get()),
            _ => "".to_string(),
        }
    }

    // This shows the control's name.
    fn get_parameter_name(&self, index: i32) -> String {
        match index {
            0 => "Gain",
            1 => "Master",
            2 => "A",
            3 => "B",
            4 => "A/B Mix",
            _ => "",
        }
        .to_string()
    }
}

// This part is important!  Without it, our plugin won't work.
plugin_main!(GainEffect);
