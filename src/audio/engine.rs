use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

use crate::models::{AudioDeviceSelection, EngineCommand, EngineEvent};

use super::decoder::DecodedSound;
use super::devices::{find_input_device, find_output_device};

const ACTIVE_EFFECT_LIMIT: usize = 32;
const MIC_RING_CAPACITY: usize = 8192;

#[derive(Clone, Copy)]
enum CallbackCommand {
    PlaySound(u64),
    StopAll,
    SetMasterVolume(f32),
    Shutdown,
}

#[derive(Clone, Copy)]
struct ActiveEffect {
    sound_index: usize,
    position: usize,
    volume: f32,
    active: bool,
}

impl ActiveEffect {
    const fn empty() -> Self {
        Self {
            sound_index: 0,
            position: 0,
            volume: 1.0,
            active: false,
        }
    }
}

pub struct AudioEngine {
    _streams: Vec<Stream>,
}

fn spawn_command_fanout(
    commands: Receiver<EngineCommand>,
    headphone_commands: Sender<CallbackCommand>,
    virtual_commands: Sender<CallbackCommand>,
    events: Sender<EngineEvent>,
) {
    thread::spawn(move || loop {
        match commands.recv() {
            Ok(command) => {
                let callback_command = match command {
                    EngineCommand::PlaySound { sound_id } => CallbackCommand::PlaySound(sound_id),
                    EngineCommand::StopAll => CallbackCommand::StopAll,
                    EngineCommand::SetMasterVolume { volume } => {
                        CallbackCommand::SetMasterVolume(volume)
                    }
                    EngineCommand::Shutdown => {
                        let _ = headphone_commands.try_send(CallbackCommand::Shutdown);
                        let _ = virtual_commands.try_send(CallbackCommand::Shutdown);
                        break;
                    }
                    EngineCommand::SetDevices(_) | EngineCommand::ReloadSounds { .. } => {
                        let _ = events.try_send(EngineEvent::Error(
                            "Configuration command must be handled by the audio supervisor"
                                .to_string(),
                        ));
                        continue;
                    }
                };
                let _ = headphone_commands.try_send(callback_command);
                let _ = virtual_commands.try_send(callback_command);
            }
            Err(_) => break,
        }
    });
}

impl AudioEngine {
    pub fn start(
        commands: Receiver<EngineCommand>,
        events: Sender<EngineEvent>,
        selections: AudioDeviceSelection,
        sound_bank: Arc<Vec<DecodedSound>>,
    ) -> Result<Self, String> {
        let host = cpal::default_host();
        let input_device = find_input_device(&host, selections.input.as_deref());
        let headphone_device = find_output_device(&host, selections.headphones.as_deref());
        let virtual_device = selections
            .virtual_microphone
            .as_deref()
            .and_then(|name| find_output_device(&host, Some(name)));

        if virtual_device.is_none() {
            let _ = events.try_send(EngineEvent::VirtualCableUnavailable);
        }
        if input_device.is_none() {
            let _ = events.try_send(EngineEvent::InputDeviceUnavailable);
        }

        let mic_ring = HeapRb::<f32>::new(MIC_RING_CAPACITY);
        let (mic_producer, mic_consumer) = mic_ring.split();

        let master_volume = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
        let (headphone_command_sender, headphone_command_receiver) = crossbeam_channel::unbounded();
        let (virtual_command_sender, virtual_command_receiver) = crossbeam_channel::unbounded();

        spawn_command_fanout(
            commands,
            headphone_command_sender,
            virtual_command_sender,
            events.clone(),
        );

        let mut streams = Vec::new();

        if let Some(device) = input_device {
            match build_input_stream(&device, mic_producer, events.clone()) {
                Ok(stream) => streams.push(stream),
                Err(error) => {
                    let _ = events.try_send(EngineEvent::Error(error));
                }
            }
        }

        if let Some(device) = headphone_device {
            match build_headphone_stream(
                &device,
                sound_bank.clone(),
                master_volume.clone(),
                headphone_command_receiver,
                events.clone(),
            ) {
                Ok(stream) => streams.push(stream),
                Err(error) => {
                    let _ = events.try_send(EngineEvent::Error(error));
                }
            }
        }

        if let Some(device) = virtual_device {
            match build_virtual_stream(
                &device,
                sound_bank,
                mic_consumer,
                master_volume,
                virtual_command_receiver,
                events.clone(),
            ) {
                Ok(stream) => streams.push(stream),
                Err(error) => {
                    let _ = events.try_send(EngineEvent::Error(error));
                }
            }
        }

        for stream in &streams {
            if let Err(error) = stream.play() {
                let _ = events.try_send(EngineEvent::Error(error.to_string()));
            }
        }

        Ok(Self { _streams: streams })
    }
}

fn build_input_stream(
    device: &cpal::Device,
    mut producer: HeapProd<f32>,
    events: Sender<EngineEvent>,
) -> Result<Stream, String> {
    let config = device
        .default_input_config()
        .map_err(|error| error.to_string())?;
    if config.sample_format() != SampleFormat::F32 {
        return Err("Default input device does not expose f32 samples".to_string());
    }

    let stream_config: StreamConfig = config.clone().into();
    let channels = stream_config.channels as usize;
    device
        .build_input_stream(
            &stream_config,
            move |samples: &[f32], _| {
                if channels == 1 {
                    for &sample in samples {
                        let _ = producer.try_push(sample);
                    }
                } else {
                    let mut frame = 0;
                    while frame + channels <= samples.len() {
                        let mut sum = 0.0_f32;
                        for channel in 0..channels {
                            sum += samples[frame + channel];
                        }
                        let mono = sum / channels as f32;
                        let _ = producer.try_push(mono);
                        frame += channels;
                    }
                }
            },
            move |_| {
                let _ = events.try_send(EngineEvent::InputDeviceUnavailable);
            },
            None,
        )
        .map_err(|error| error.to_string())
}

fn build_headphone_stream(
    device: &cpal::Device,
    sound_bank: Arc<Vec<DecodedSound>>,
    master_volume: Arc<AtomicU32>,
    commands: Receiver<CallbackCommand>,
    events: Sender<EngineEvent>,
) -> Result<Stream, String> {
    let config = device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    if config.sample_format() != SampleFormat::F32 {
        return Err("Default output device does not expose f32 samples".to_string());
    }

    let stream_config: StreamConfig = config.clone().into();
    let channels = stream_config.channels as usize;
    let mut active = [ActiveEffect::empty(); ACTIVE_EFFECT_LIMIT];

    device
        .build_output_stream(
            &stream_config,
            move |samples: &mut [f32], _| {
                fill_headphones(
                    samples,
                    channels,
                    &sound_bank,
                    &master_volume,
                    &commands,
                    &mut active,
                );
            },
            move |_| {
                let _ = events.try_send(EngineEvent::OutputDeviceUnavailable);
            },
            None,
        )
        .map_err(|error| error.to_string())
}

fn fill_headphones(
    samples: &mut [f32],
    channels: usize,
    sound_bank: &[DecodedSound],
    master_volume: &AtomicU32,
    commands: &Receiver<CallbackCommand>,
    active: &mut [ActiveEffect; ACTIVE_EFFECT_LIMIT],
) {
    loop {
        match commands.try_recv() {
            Ok(CallbackCommand::PlaySound(sound_id)) => {
                if let Some(index) = sound_bank.iter().position(|sound| sound.id == sound_id) {
                    if let Some(slot) = active.iter_mut().find(|effect| !effect.active) {
                        slot.sound_index = index;
                        slot.position = 0;
                        slot.volume = sound_bank[index].volume;
                        slot.active = true;
                    }
                }
            }
            Ok(CallbackCommand::StopAll) => {
                for effect in active.iter_mut() {
                    effect.active = false;
                }
            }
            Ok(CallbackCommand::SetMasterVolume(volume)) => {
                master_volume.store(volume.to_bits(), Ordering::Relaxed);
            }
            Ok(CallbackCommand::Shutdown) => {
                for effect in active.iter_mut() {
                    effect.active = false;
                }
            }
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
        }
    }

    let master = f32::from_bits(master_volume.load(Ordering::Relaxed)).clamp(0.0, 1.0);
    let mut frame = 0;
    while frame + channels <= samples.len() {
        let mut left = 0.0_f32;
        let mut right = 0.0_f32;

        for effect in active.iter_mut() {
            if !effect.active {
                continue;
            }
            if let Some(sound) = sound_bank.get(effect.sound_index) {
                if let Some(frame_samples) = sound.stereo_frames.get(effect.position) {
                    left += frame_samples[0] * effect.volume * master;
                    right += frame_samples[1] * effect.volume * master;
                    effect.position += 1;
                } else {
                    effect.active = false;
                }
            } else {
                effect.active = false;
            }
        }

        for channel in 0..channels {
            samples[frame + channel] = if channel == 0 || channels == 1 {
                left
            } else {
                right
            };
        }
        frame += channels;
    }
}

fn build_virtual_stream(
    device: &cpal::Device,
    sound_bank: Arc<Vec<DecodedSound>>,
    mut mic_consumer: HeapCons<f32>,
    master_volume: Arc<AtomicU32>,
    commands: Receiver<CallbackCommand>,
    events: Sender<EngineEvent>,
) -> Result<Stream, String> {
    let config = device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    if config.sample_format() != SampleFormat::F32 {
        return Err("Default virtual output does not expose f32 samples".to_string());
    }

    let stream_config: StreamConfig = config.clone().into();
    let channels = stream_config.channels as usize;
    let mut active = [ActiveEffect::empty(); ACTIVE_EFFECT_LIMIT];

    device
        .build_output_stream(
            &stream_config,
            move |samples: &mut [f32], _| {
                fill_virtual(
                    samples,
                    channels,
                    &sound_bank,
                    &master_volume,
                    &mut mic_consumer,
                    &commands,
                    &mut active,
                );
            },
            move |_| {
                let _ = events.try_send(EngineEvent::VirtualCableUnavailable);
            },
            None,
        )
        .map_err(|error| error.to_string())
}

fn fill_virtual(
    samples: &mut [f32],
    channels: usize,
    sound_bank: &[DecodedSound],
    master_volume: &AtomicU32,
    mic_consumer: &mut HeapCons<f32>,
    commands: &Receiver<CallbackCommand>,
    active: &mut [ActiveEffect; ACTIVE_EFFECT_LIMIT],
) {
    loop {
        match commands.try_recv() {
            Ok(CallbackCommand::PlaySound(sound_id)) => {
                if let Some(index) = sound_bank.iter().position(|sound| sound.id == sound_id) {
                    if let Some(slot) = active.iter_mut().find(|effect| !effect.active) {
                        slot.sound_index = index;
                        slot.position = 0;
                        slot.volume = sound_bank[index].volume;
                        slot.active = true;
                    }
                }
            }
            Ok(CallbackCommand::StopAll) => {
                for effect in active.iter_mut() {
                    effect.active = false;
                }
            }
            Ok(CallbackCommand::SetMasterVolume(volume)) => {
                master_volume.store(volume.to_bits(), Ordering::Relaxed);
            }
            Ok(CallbackCommand::Shutdown) => {
                for effect in active.iter_mut() {
                    effect.active = false;
                }
            }
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
        }
    }

    let master = f32::from_bits(master_volume.load(Ordering::Relaxed)).clamp(0.0, 1.0);
    let mut frame = 0;
    while frame + channels <= samples.len() {
        let microphone = match mic_consumer.try_pop() {
            Some(sample) => sample,
            None => 0.0,
        };
        let mut left = microphone;
        let mut right = microphone;

        for effect in active.iter_mut() {
            if !effect.active {
                continue;
            }
            if let Some(sound) = sound_bank.get(effect.sound_index) {
                if let Some(frame_samples) = sound.stereo_frames.get(effect.position) {
                    left += frame_samples[0] * effect.volume * master;
                    right += frame_samples[1] * effect.volume * master;
                    effect.position += 1;
                } else {
                    effect.active = false;
                }
            } else {
                effect.active = false;
            }
        }

        left = left / (1.0 + left.abs());
        right = right / (1.0 + right.abs());

        for channel in 0..channels {
            samples[frame + channel] = if channel == 0 || channels == 1 {
                left
            } else {
                right
            };
        }
        frame += channels;
    }
}
