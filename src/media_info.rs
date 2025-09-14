use ffmpeg_next as ffmpeg;
use ffmpeg::{format, media, codec, Rational};
use std::collections::HashMap;
use ffmpeg_next::codec::{Capabilities, Profile};
use ffmpeg_next::{color, ChannelLayout};

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub format_name: String,
    pub format_description: String,
    pub duration_ms: i64,
    pub bit_rate: Option<usize>,
    pub video_streams: Vec<VideoStreamInfo>,
    pub audio_streams: Vec<AudioStreamInfo>,
    pub subtitle_streams: Vec<SubtitleStreamInfo>,
    pub other_streams: Vec<OtherStreamInfo>,
    pub chapters: Vec<ChapterInfo>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct VideoStreamInfo {
    pub index: usize,
    pub codec_name: String,
    pub codec_id: String,
    pub codec_description: String,
    pub codec_capabilities: Option<Capabilities>,
    pub codec_profiles: Option<Vec<Profile>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub pixel_format: Option<format::Pixel>,
    pub frame_rate: Option<RationalValue>,
    pub bit_rate: Option<usize>,
    pub frames: Option<u64>,
    pub color_space: Option<color::space::Space>,
    pub aspect_ratio: Option<RationalValue>,
    pub time_base: RationalValue,
    pub disposition: u32,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AudioStreamInfo {
    pub index: usize,
    pub codec_name: String,
    pub codec_id: String,
    pub codec_description: String,
    pub codec_capabilities: Option<Capabilities>,
    pub codec_profiles: Option<Vec<Profile>>,
    pub channels: Option<u16>,
    pub sample_rate: Option<u32>,
    pub sample_format: Option<format::Sample>,
    pub bit_rate: Option<usize>,
    pub channel_layout: Option<ChannelLayout>,
    pub frames: Option<u64>,
    pub time_base: RationalValue,
    pub disposition: u32,
    pub profile: Option<Profile>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SubtitleStreamInfo {
    pub index: usize,
    pub codec_name: String,
    pub codec_id: String,
    pub language: Option<String>,
    pub time_base: RationalValue,
    pub disposition: u32,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct OtherStreamInfo {
    pub index: usize,
    pub codec_name: String,
    pub codec_id: String,
    pub stream_type: String,
    pub time_base: RationalValue,
    pub disposition: u32,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ChapterInfo {
    pub index: usize,
    pub title: String,
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RationalValue {
    pub numerator: i32,
    pub denominator: i32,
    pub value: f64,
}

impl From<Rational> for RationalValue {
    fn from(rational: Rational) -> Self {
        RationalValue {
            numerator: rational.numerator(),
            denominator: rational.denominator(),
            value: if rational.denominator() != 0 {
                rational.numerator() as f64 / rational.denominator() as f64
            } else {
                0.0
            },
        }
    }
}

pub fn get_media_info(filename: &str) -> Option<MediaInfo> {
    match ffmpeg::init() {
        Ok(_) => {},
        Err(_) => {
            return None;
        }
    };

    let input = match format::input(&filename) {
        Ok(i) => i,
        Err(_) => {
            return None;
        }
    };

    let mut info = MediaInfo {
        format_name: input.format().name().to_string(),
        format_description: input.format().description().to_string(),
        duration_ms: input.duration(),
        bit_rate: Some(0),
        video_streams: Vec::new(),
        audio_streams: Vec::new(),
        subtitle_streams: Vec::new(),
        other_streams: Vec::new(),
        chapters: Vec::new(),
        metadata: input.metadata().iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    };

    for (index, stream) in input.streams().enumerate() {
        let codec_params = stream.parameters();
        let codec_id = codec_params.id();
        let codec = codec::decoder::find(codec_id);

        let codec_name = match codec {
            Some(c) => c.name().to_string(),
            None => "Unknown".to_string(),
        };

        let codec_description = match codec {
            Some(c) => c.description().to_string(),
            None => "Unknown".to_string(),
        };

        let codec_capabilities = match codec {
            Some(c) => {
                Some(c.capabilities())
            },
            None => None,
        };

         let codec_profiles = match codec {
            Some(c) => {
                let iterator = c.profiles();

                match iterator {
                    Some(i) => {
                        Some(i.map(|p| p).collect::<Vec<_>>())
                    },
                    None => None
                }
            },
            None => None,
        };

        let codec_id_str = format!("{:?}", codec_id);
        let time_base = RationalValue::from(stream.time_base());
        let disposition = stream.disposition();
        let metadata: HashMap<String, String> = stream.metadata().iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        match stream.parameters().medium() {
            media::Type::Video => {
                let video = input.stream(index).unwrap();
                let video_params = video.parameters();
                let context = ffmpeg::codec::Context::from_parameters(video_params).unwrap();
                let decoder = context.decoder().video().unwrap();
                
                
                let mut vs_info = VideoStreamInfo {
                    index,
                    codec_name,
                    codec_description,
                    codec_capabilities,
                    codec_profiles,
                    codec_id: codec_id_str,
                    width: None,
                    height: None,
                    pixel_format: None,
                    frame_rate: Some(RationalValue::from(stream.avg_frame_rate())),
                    frames: estimate_frame_count(&filename, index),
                    bit_rate: None,
                    color_space: None,
                    aspect_ratio: None,
                    time_base,
                    disposition: disposition.bits() as u32,
                    metadata,
                };

                vs_info.width = Some(decoder.width());
                vs_info.height = Some(decoder.height());
                vs_info.pixel_format = Some(decoder.format());
                vs_info.color_space = Some(decoder.color_space());
                vs_info.bit_rate = Some(decoder.bit_rate());
                vs_info.aspect_ratio = Some(RationalValue::from(decoder.aspect_ratio()));

                info.video_streams.push(vs_info);
            },
            media::Type::Audio => {
                let mut as_info = AudioStreamInfo {
                    index,
                    codec_name,
                    codec_description,
                    codec_capabilities,
                    codec_profiles,
                    codec_id: codec_id_str,
                    channels: None,
                    sample_rate: None,
                    sample_format: None,
                    channel_layout: None,
                    frames: estimate_frame_count(&filename, index),
                    bit_rate: None,
                    time_base,
                    profile: None,
                    disposition: disposition.bits() as u32,
                    metadata,
                };
                
                let audio = input.stream(index).unwrap();
                let audio_params = audio.parameters();
                let context = ffmpeg::codec::Context::from_parameters(audio_params).unwrap();
                let decoder = context.decoder().audio().unwrap();
                
                as_info.channels = Some(decoder.channels());
                as_info.sample_rate = Some(decoder.rate());
                as_info.sample_format = Some(decoder.format());
                as_info.channel_layout = Some(decoder.channel_layout());
                as_info.profile = Some(decoder.profile());
                as_info.bit_rate = Some(decoder.bit_rate());

                info.audio_streams.push(as_info);
            },
            media::Type::Subtitle => {
                let language = metadata.get("language").cloned();

                let ss_info = SubtitleStreamInfo {
                    index,
                    codec_name,
                    codec_id: codec_id_str,
                    language,
                    time_base,
                    disposition: disposition.bits() as u32,
                    metadata,
                };

                info.subtitle_streams.push(ss_info);
            },
            other_medium => {
                let os_info = OtherStreamInfo {
                    index,
                    codec_name,
                    codec_id: codec_id_str,
                    stream_type: format!("{:?}", other_medium),
                    time_base,
                    disposition: disposition.bits() as u32,
                    metadata,
                };

                info.other_streams.push(os_info);
            }
        }
    }

    Some(info)
}

fn estimate_frame_count(filename: &str, stream_index: usize) -> Option<u64> {
    let mut input = match format::input(&filename) {
        Ok(i) => i,
        Err(_) => {
            return None;
        }
    };
    
    let frames = input.streams().nth(stream_index)?.frames();
    
    if frames > 0 {
        return Some(frames as u64);
    }

    let mut frame_count = 0;

    if input.seek(0, ..0).is_err() {
        return None;
    }

    for (stream, _) in input.packets() {
        if stream.index() == stream_index {
            frame_count += 1;
        }
    }

    let _ = input.seek(0, ..0);

    Some(frame_count)
}