use std::fs::File;
use std::path::Path;

use rubato::{FftFixedIn, Resampler};
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::models::SoundId;

#[derive(Debug, Clone)]
pub struct DecodedSound {
    pub id: SoundId,
    pub volume: f32,
    pub stereo_frames: Vec<[f32; 2]>,
}

fn append_buffer(input: AudioBufferRef<'_>, samples: &mut Vec<f32>) {
    match input {
        AudioBufferRef::U8(buffer) => {
            let frames = buffer.frames();
            let channels = buffer.spec().channels.count();
            for frame in 0..frames {
                append_channels(samples, channels, |channel| {
                    let value = buffer.chan(channel)[frame];
                    (value as f32 - 128.0) / 128.0
                });
            }
        }
        AudioBufferRef::U16(buffer) => {
            let frames = buffer.frames();
            let channels = buffer.spec().channels.count();
            for frame in 0..frames {
                append_channels(samples, channels, |channel| {
                    let value = buffer.chan(channel)[frame];
                    (value as f32 - 32768.0) / 32768.0
                });
            }
        }
        AudioBufferRef::U24(buffer) => {
            let frames = buffer.frames();
            let channels = buffer.spec().channels.count();
            for frame in 0..frames {
                append_channels(samples, channels, |channel| {
                    buffer.chan(channel)[frame].inner() as f32 / 8388608.0
                });
            }
        }
        AudioBufferRef::U32(buffer) => {
            let frames = buffer.frames();
            let channels = buffer.spec().channels.count();
            for frame in 0..frames {
                append_channels(samples, channels, |channel| {
                    let value = buffer.chan(channel)[frame];
                    (value as f32 - 2147483648.0) / 2147483648.0
                });
            }
        }
        AudioBufferRef::S16(buffer) => {
            let frames = buffer.frames();
            let channels = buffer.spec().channels.count();
            for frame in 0..frames {
                append_channels(samples, channels, |channel| {
                    buffer.chan(channel)[frame] as f32 / 32768.0
                });
            }
        }
        AudioBufferRef::S8(buffer) => {
            let frames = buffer.frames();
            let channels = buffer.spec().channels.count();
            for frame in 0..frames {
                append_channels(samples, channels, |channel| {
                    buffer.chan(channel)[frame] as f32 / 128.0
                });
            }
        }
        AudioBufferRef::S24(buffer) => {
            let frames = buffer.frames();
            let channels = buffer.spec().channels.count();
            for frame in 0..frames {
                append_channels(samples, channels, |channel| {
                    buffer.chan(channel)[frame].inner() as f32 / 8388608.0
                });
            }
        }
        AudioBufferRef::S32(buffer) => {
            let frames = buffer.frames();
            let channels = buffer.spec().channels.count();
            for frame in 0..frames {
                append_channels(samples, channels, |channel| {
                    buffer.chan(channel)[frame] as f32 / 2147483648.0
                });
            }
        }
        AudioBufferRef::F32(buffer) => {
            let frames = buffer.frames();
            let channels = buffer.spec().channels.count();
            for frame in 0..frames {
                append_channels(samples, channels, |channel| buffer.chan(channel)[frame]);
            }
        }
        AudioBufferRef::F64(buffer) => {
            let frames = buffer.frames();
            let channels = buffer.spec().channels.count();
            for frame in 0..frames {
                append_channels(samples, channels, |channel| {
                    buffer.chan(channel)[frame] as f32
                });
            }
        }
    }
}

fn append_channels(samples: &mut Vec<f32>, channels: usize, mut value: impl FnMut(usize) -> f32) {
    let mut left = 0.0_f32;
    let mut right = 0.0_f32;

    if channels == 1 {
        left = value(0);
        right = left;
    } else if channels >= 2 {
        left = value(0);
        right = value(1);
    }

    samples.push(left);
    samples.push(right);
}

pub fn decode_and_resample(
    id: SoundId,
    volume: f32,
    path: &Path,
    output_sample_rate: u32,
) -> Result<DecodedSound, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let media_source = MediaSourceStream::new(
        Box::new(file),
        symphonia::core::io::MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            media_source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| error.to_string())?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| "No decodable audio track found".to_string())?;
    let track_id = track.id;
    let input_sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| "Missing source sample rate".to_string())?;

    let mut decoder = symphonia::default::get_codecs()
        .make(
            &track.codec_params,
            &symphonia::core::codecs::DecoderOptions::default(),
        )
        .map_err(|error| error.to_string())?;

    let mut interleaved = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => return Err(error.to_string()),
        };
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(buffer) => append_buffer(buffer, &mut interleaved),
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(error) => return Err(error.to_string()),
        }
    }

    if interleaved.is_empty() {
        return Ok(DecodedSound {
            id,
            volume,
            stereo_frames: Vec::new(),
        });
    }

    let mut left = Vec::with_capacity(interleaved.len() / 2);
    let mut right = Vec::with_capacity(interleaved.len() / 2);
    for frame in interleaved.chunks_exact(2) {
        left.push(frame[0]);
        right.push(frame[1]);
    }

    let stereo_frames = if input_sample_rate == output_sample_rate {
        left.into_iter()
            .zip(right)
            .map(|(left, right)| [left, right])
            .collect()
    } else {
        let chunk_size = 1024;
        let mut resampler = FftFixedIn::<f32>::new(
            input_sample_rate as usize,
            output_sample_rate as usize,
            chunk_size,
            2,
            2,
        )
        .map_err(|error| error.to_string())?;

        let mut out_left = Vec::new();
        let mut out_right = Vec::new();
        let frame_count = left.len().max(right.len());
        let mut frame = 0;
        while frame < frame_count {
            let end = (frame + chunk_size).min(frame_count);
            let mut left_chunk = vec![0.0_f32; chunk_size];
            let mut right_chunk = vec![0.0_f32; chunk_size];
            if let Some(source) = left.get(frame..end) {
                left_chunk[..source.len()].copy_from_slice(source);
            }
            if let Some(source) = right.get(frame..end) {
                right_chunk[..source.len()].copy_from_slice(source);
            }
            let input = vec![left_chunk, right_chunk];
            let mut processed = resampler
                .process(&input, None)
                .map_err(|error| error.to_string())?;
            if processed.len() >= 2 {
                out_left.append(&mut processed[0]);
                out_right.append(&mut processed[1]);
            }
            frame = end;
        }

        out_left
            .into_iter()
            .zip(out_right)
            .map(|(left, right)| [left, right])
            .collect()
    };

    Ok(DecodedSound {
        id,
        volume,
        stereo_frames,
    })
}
