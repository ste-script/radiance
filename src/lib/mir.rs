pub use crate::beat_tracking::AudioLevels;
use crate::beat_tracking::{BeatTracker, N_FILTERS, SAMPLE_RATE};
use cpal::traits::{DeviceTrait, HostTrait};
use std::sync::mpsc;
use std::time;

const MAX_TIME: f32 = 64.;

const DEFAULT_BPM: f32 = 120.;

// These don't necessarily have to match the beat tracking HMM
// but the HMM parameters are a good starting point
const MIN_BPS: f32 = 55. / 60.;
const MAX_BPS: f32 = 215. / 60.;

pub const SPECTRUM_LENGTH: usize = N_FILTERS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InputDeviceKind {
    Microphone,
    Loopback,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioInputDevice {
    pub name: String,
    pub kind: InputDeviceKind,
}

impl AudioInputDevice {
    fn from_name(name: String) -> Self {
        Self {
            kind: InputDeviceKind::classify(&name),
            name,
        }
    }
}

impl InputDeviceKind {
    fn classify(name: &str) -> Self {
        let normalized_name = name.to_ascii_lowercase();
        let loopback_keywords = [
            "monitor",
            "loopback",
            "stereo mix",
            "what u hear",
            "blackhole",
            "soundflower",
            "vb-audio",
            "cable output",
        ];
        if loopback_keywords
            .iter()
            .any(|keyword| normalized_name.contains(keyword))
        {
            return Self::Loopback;
        }

        let microphone_keywords = [
            "microphone",
            " mic",
            "mic ",
            "headset",
            "webcam",
            "built-in input",
            "built in input",
            "internal mic",
            "array",
        ];
        if normalized_name == "mic"
            || microphone_keywords
                .iter()
                .any(|keyword| normalized_name.contains(keyword))
        {
            return Self::Microphone;
        }

        Self::Other
    }
}

/// A Mir (Music information retrieval) object
/// handles listening to a live audio input device, such as a
/// microphone or loopback/monitor input,
/// and generates a global timebase from the beats,
/// along with other relevant real-time inputs based on the music
/// such as the low, mid, high, and overall audio level.
/// It can be polled from the main thread at every frame,
/// at which point it will compute and return the current time in beats,
/// and the four audio levels.
/// It has a buffer of about a second or so,
/// but should be polled frequently to avoid dropped updates.
pub struct Mir {
    _stream: Option<cpal::Stream>,
    receiver: mpsc::Receiver<Update>,
    last_update: Update,
    pub global_timescale: f32,
    pub latency_compensation: f32, // Anticipate beats by this many seconds
    selected_device_name: Option<String>,
}

/// Updates sent over a queue
/// from the audio thread to the main thread
/// containing new data from which to produce MusicInfos
/// when polled
#[derive(Clone, Debug)]
struct Update {
    // For computing t in beats
    // We will do a linear calculation
    // of the form t = tempo * (x - wall_ref) + t_ref
    // x (the wallclock time measured at the time of poll(); an Instant)
    // and wall_ref (a reference Instant in wallclock time)
    // yield (x - wall_ref) (a duration in seconds.)
    // m is a tempo in beats per second.
    wall_ref: time::Instant, // reference wall clock time
    t_ref: f32,              // reference t measured in beats
    tempo: f32,              // beats per second

    // For computing the audio levels
    audio: AudioLevels,
    spectrum: [f32; SPECTRUM_LENGTH],
    beat_locked: bool,
}

impl Update {
    fn t(&self, wall: time::Instant) -> f32 {
        let elapsed = (wall - self.wall_ref).as_secs_f32();
        let t = self.tempo * elapsed + self.t_ref;
        t.rem_euclid(MAX_TIME * MAX_TIME) // MAX_TIME^2 is sort of arbitrary, just don't let it grow too big
    }
}

/// The structure returned from the Mir object when it is polled,
/// containing real-time information about the audio.
#[derive(Clone, Debug)]
pub struct MusicInfo {
    pub time: f32,          // time in beats
    pub unscaled_time: f32, // time in beats, without the global timescale applied
    // (for widgets that shouldn't be affected by global
    // timescale)
    pub uncompensated_unscaled_time: f32, // time in beats, without the global timescale or latency compensation (so it aligns with reported audio levels)
    pub tempo: f32,                       // beats per second
    pub audio: AudioLevels,
    pub spectrum: [f32; SPECTRUM_LENGTH],
    pub beat_locked: bool,
}

impl Default for Mir {
    fn default() -> Self {
        Self::new()
    }
}

impl Mir {
    fn default_update() -> Update {
        Update {
            wall_ref: time::Instant::now(),
            t_ref: 0.,
            tempo: DEFAULT_BPM / 60.,
            audio: Default::default(),
            spectrum: [0.; SPECTRUM_LENGTH],
            beat_locked: false,
        }
    }

    fn audio_input(
        sender: mpsc::SyncSender<Update>,
        device_name: Option<&str>,
    ) -> Result<cpal::Stream, String> {
        use cpal::traits::StreamTrait;

        let host = cpal::default_host();
        let device = match device_name {
            Some(name) => host
                .input_devices()
                .map_err(|e| format!("Failed to enumerate input devices: {:?}", e))?
                .find(|d| {
                    d.description()
                        .ok()
                        .map(|desc| desc.name().to_string())
                        .as_deref()
                        == Some(name)
                })
                .ok_or_else(|| format!("Audio input device '{}' not found", name))?,
            None => host
                .default_input_device()
                .ok_or("No audio input devices found")?,
        };

        const MIN_USEFUL_BUFFER_SIZE: cpal::FrameCount = 256; // Lower actually would be useful, but CPAL lies about the min size, so this ought to be safe
        const SAMPLE_RATE_CPAL: cpal::SampleRate = SAMPLE_RATE as u32;
        let supported_input_configs = device.supported_input_configs().map_err(|e| {
            format!(
                "Could not query audio device for supported input configs: {:?}",
                e
            )
        })?;

        let config_range = supported_input_configs
            .filter(|config| {
                (config.sample_format() == cpal::SampleFormat::I16
                    || config.sample_format() == cpal::SampleFormat::U16
                    || config.sample_format() == cpal::SampleFormat::F32)
                    && SAMPLE_RATE_CPAL >= config.min_sample_rate()
                    && SAMPLE_RATE_CPAL <= config.max_sample_rate()
                    && match *config.buffer_size() {
                        cpal::SupportedBufferSize::Range { max, .. } => {
                            MIN_USEFUL_BUFFER_SIZE <= max
                        }
                        cpal::SupportedBufferSize::Unknown => true,
                    }
                    && config.channels() >= 1
            })
            .min_by_key(|config| match *config.buffer_size() {
                cpal::SupportedBufferSize::Range { min, .. } => MIN_USEFUL_BUFFER_SIZE.max(min),
                cpal::SupportedBufferSize::Unknown => 8192, // Large but not unreasonable
            })
            .ok_or_else(|| {
                let supported_input_configs_str = device
                    .supported_input_configs()
                    .unwrap()
                    .map(|c| format!("{:?}", c))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "No supported audio input configs were found. Options were: {}",
                    supported_input_configs_str,
                )
            })?
            .with_sample_rate(SAMPLE_RATE_CPAL);

        let mut config = config_range.config();

        if let cpal::SupportedBufferSize::Range { min, .. } = *config_range.buffer_size() {
            config.buffer_size = cpal::BufferSize::Fixed(MIN_USEFUL_BUFFER_SIZE.max(min));
        }

        println!("MIR: Choosing audio config: {:?}", config);

        // This tempo will be quickly overridden as the audio thread
        // starts tapping out the real beat
        let mut update = Update {
            wall_ref: time::Instant::now(),
            t_ref: 0.,
            tempo: DEFAULT_BPM / 60.,
            audio: Default::default(),
            spectrum: [0.; SPECTRUM_LENGTH],
            beat_locked: false,
        };

        // Make a new beat tracker
        let mut bt = BeatTracker::new();

        let mut process_audio_i16_mono = move |data: &[i16]| {
            // Reduce all of the returned results into just the most recent
            // Typically; only 0 or 1 results are returned per audio frame,
            // but we do this reduction just to be safe,
            // in case the audio frames returned are really large
            let recent_result = bt.process(data).into_iter().reduce(
                |(_, _, _, beat_acc, _), (audio, spectrum, activation, beat, beat_locked)| {
                    (audio, spectrum, activation, beat_acc || beat, beat_locked)
                },
            );
            let (audio, spectrum, _activation, beat, beat_locked) = match recent_result {
                Some(result) => result,
                None => {
                    return;
                }
            };

            // Compute the update
            // If we detected a beat, recompute the linear parameters for t
            if beat {
                // In computing the new line, we want to preserve continuity;
                // i.e. we want to pivot our line about the current point (wall clock time, current t in beats)
                // So, we set wall_ref to right now, and t_ref to t(wall_ref)
                let wall_ref = time::Instant::now();
                let t_ref = update.t(wall_ref);

                // Now we just have one remaining parameter to set: the slope (aka tempo)
                // We set the slope of the line so that it intersects the point
                // (expected wall clock time of next beat, current integer beat + 1)

                // Inter-arrival time of the last two beats, in seconds
                let last_beat_wall_period = (wall_ref - update.wall_ref).as_secs_f32();
                // Next beat
                let next_beat = t_ref.round() + 1.0;
                // Amount of ground we need to cover, in number of beats
                let beats_to_cover = next_beat - t_ref;
                // Typically, beats_to_cover should be close to 1.0 if we're doing a good job.
                let tempo = beats_to_cover / last_beat_wall_period;

                // Only update the tempo if it's a reasonable value,
                if (MIN_BPS..=MAX_BPS).contains(&tempo) {
                    update.tempo = tempo;
                }

                update.wall_ref = wall_ref;
                update.t_ref = t_ref;
            }

            update.audio = audio;
            update.spectrum = spectrum.data.as_vec().to_vec().try_into().unwrap();
            update.beat_locked = beat_locked;

            // Send an update back to the main thread
            if let Err(err) = sender.try_send(update.clone()) {
                match err {
                    mpsc::TrySendError::Full(_) => {
                        // Expected when audio callbacks outpace UI polling; harmless
                    }
                    mpsc::TrySendError::Disconnected(_) => {
                        println!("MIR: main thread disconnected; dropping update");
                    }
                }
            };
        };

        let process_error = move |err| println!("MIR: audio stream error: {:?}", err);

        let channels = config.channels as usize;

        let stream = match config_range.sample_format() {
            cpal::SampleFormat::I16 => device
                .build_input_stream(
                    &config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let data: Vec<i16> = data
                            .chunks(channels)
                            .map(|frame| {
                                let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                                (sum / channels as i32) as i16
                            })
                            .collect();
                        process_audio_i16_mono(&data);
                    },
                    process_error,
                    None,
                )
                .map_err(|e| format!("Failed to construct audio input stream: {:?}", e)),
            cpal::SampleFormat::U16 => device
                .build_input_stream(
                    &config,
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        let data: Vec<i16> = data
                            .chunks(channels)
                            .map(|frame| {
                                let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                                (sum / channels as i32 - 32768) as i16
                            })
                            .collect();
                        process_audio_i16_mono(&data);
                    },
                    process_error,
                    None,
                )
                .map_err(|e| format!("Failed to construct audio input stream: {:?}", e)),
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let data: Vec<i16> = data
                            .chunks(channels)
                            .map(|frame| {
                                let sum: f32 = frame.iter().sum();
                                (sum / channels as f32 * 32767.) as i16
                            })
                            .collect();
                        process_audio_i16_mono(&data);
                    },
                    process_error,
                    None,
                )
                .map_err(|e| format!("Failed to construct audio input stream: {:?}", e)),
            s => Err(format!(
                "Unexpected sample format (s={s:?}, must be I16, U16, or F32)"
            )),
        }?;

        // Start the stream
        stream
            .play()
            .map_err(|e| format!("Failed to start audio stream: {:?}", e))?;

        Ok(stream)
    }

    pub fn new() -> Self {
        Self::new_with_device(None)
    }

    pub fn new_with_device(device_name: Option<String>) -> Self {
        // Make a communication channel to communicate with the audio thread
        const MESSAGE_BUFFER_SIZE: usize = 16;
        let (sender, receiver) = mpsc::sync_channel(MESSAGE_BUFFER_SIZE);

        let requested_device_name = device_name;

        // Set up audio input
        let (stream, last_update, selected_device_name) = match Self::audio_input(
            sender.clone(),
            requested_device_name.as_deref(),
        ) {
            Ok(stream) => (
                Some(stream),
                Self::default_update(),
                requested_device_name,
            ),
            Err(e) => {
                if let Some(name) = requested_device_name.as_deref() {
                    println!("MIR: Failed to use requested audio input '{}': {}", name, e);
                    match Self::audio_input(sender, None) {
                        Ok(stream) => {
                            println!("MIR: Falling back to the default audio input");
                            (Some(stream), Self::default_update(), None)
                        }
                        Err(fallback_err) => {
                            println!("MIR: {}", fallback_err);
                            println!(
                                "MIR: Proceeding with no audio input at a constant BPM of {}",
                                DEFAULT_BPM
                            );
                            (None, Self::default_update(), None)
                        }
                    }
                } else {
                    println!("MIR: {}", e);
                    println!(
                        "MIR: Proceeding with no audio input at a constant BPM of {}",
                        DEFAULT_BPM
                    );
                    (None, Self::default_update(), None)
                }
            }
        };

        Self {
            _stream: stream,
            receiver,
            last_update,
            global_timescale: 1.,
            latency_compensation: 0.1,
            selected_device_name,
        }
    }

    pub fn available_input_devices() -> Vec<AudioInputDevice> {
        let host = cpal::default_host();
        match host.input_devices() {
            Ok(devices) => {
                let mut devices: Vec<_> = devices
                    .filter_map(|d| {
                        d.description()
                            .ok()
                            .map(|desc| AudioInputDevice::from_name(desc.name().to_string()))
                    })
                    .collect();
                devices.sort_by(|left, right| {
                    left.kind
                        .cmp(&right.kind)
                        .then_with(|| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()))
                });
                devices
            }
            Err(e) => {
                println!("MIR: Failed to enumerate input devices: {:?}", e);
                Vec::new()
            }
        }
    }

    pub fn selected_device_name_option(&self) -> Option<&str> {
        self.selected_device_name.as_deref()
    }

    pub fn selected_device_name(&self) -> &str {
        match &self.selected_device_name {
            Some(name) => name.as_str(),
            None => "Default input",
        }
    }

    pub fn switch_device(&mut self, device_name: Option<String>) {
        self.selected_device_name = device_name;

        // Drop the current stream
        self._stream = None;

        // Create a new channel and stream
        const MESSAGE_BUFFER_SIZE: usize = 16;
        let (sender, receiver) = mpsc::sync_channel(MESSAGE_BUFFER_SIZE);
        let requested_device_name = self.selected_device_name.clone();

        match Self::audio_input(sender.clone(), requested_device_name.as_deref()) {
            Ok(stream) => {
                println!(
                    "MIR: Switched to audio device: {}",
                    self.selected_device_name()
                );
                self._stream = Some(stream);
                self.receiver = receiver;
            }
            Err(e) => {
                if let Some(name) = requested_device_name.as_deref() {
                    println!("MIR: Failed to switch to audio device '{}': {}", name, e);
                    match Self::audio_input(sender, None) {
                        Ok(stream) => {
                            println!("MIR: Falling back to the default audio input");
                            self.selected_device_name = None;
                            self._stream = Some(stream);
                            self.receiver = receiver;
                        }
                        Err(fallback_err) => {
                            println!("MIR: {}", fallback_err);
                            println!("MIR: Proceeding with no audio input");
                            self.selected_device_name = None;
                            self._stream = None;
                            self.receiver = receiver;
                        }
                    }
                } else {
                    println!("MIR: Failed to switch audio device: {}", e);
                    println!("MIR: Proceeding with no audio input");
                    self.selected_device_name = None;
                    self._stream = None;
                    self.receiver = receiver;
                }
            }
        }
    }

    pub fn poll(&mut self) -> MusicInfo {
        // Drain the receiver,
        // applying the most recent update from the audio thread
        if let Some(update) = self.receiver.try_iter().last() {
            self.last_update = update;
        }

        // Compute t
        let uncompensated_unscaled_time = self
            .last_update
            .t(time::Instant::now())
            .rem_euclid(MAX_TIME);

        let unscaled_time = self
            .last_update
            .t(time::Instant::now() + time::Duration::from_secs_f32(self.latency_compensation))
            .rem_euclid(MAX_TIME);

        let time = (self
            .last_update
            .t(time::Instant::now() + time::Duration::from_secs_f32(self.latency_compensation))
            * self.global_timescale)
            .rem_euclid(MAX_TIME);

        MusicInfo {
            time,
            unscaled_time,
            uncompensated_unscaled_time,
            tempo: self.last_update.tempo * self.global_timescale,
            audio: self.last_update.audio.clone(),
            spectrum: self.last_update.spectrum.clone(),
            beat_locked: self.last_update.beat_locked,
        }
    }
}
