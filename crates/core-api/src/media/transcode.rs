//! ffmpeg → HLS transcoding (Phase 1a: a single rendition).
//!
//! Produces a VOD HLS playlist (`index.m3u8`) plus `.ts` segments from a source
//! upload. Returns the output files in memory for the caller to upload to the
//! object store. A multi-bitrate ladder (master playlist) is a later refinement.
//!
//! ffmpeg is invoked as a subprocess; it must be on PATH.

use std::path::Path;

use thiserror::Error;
use tokio::process::Command;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum TranscodeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ffmpeg failed: {0}")]
    Ffmpeg(String),
}

/// One produced HLS file (manifest or segment), ready to upload.
pub struct TranscodedFile {
    pub name: String,
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
}

pub struct TranscodeOutput {
    pub files: Vec<TranscodedFile>,
    pub manifest_name: String,
}

/// Transcode `source` to HLS. `is_video` chooses a video+audio vs audio-only
/// pipeline. Work happens in a unique temp dir that is always cleaned up.
pub async fn to_hls(source: &[u8], is_video: bool) -> Result<TranscodeOutput, TranscodeError> {
    let work = std::env::temp_dir().join(format!("gamma-transcode-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&work).await?;

    let result = run(&work, source, is_video).await;

    // Best-effort cleanup regardless of outcome.
    let _ = tokio::fs::remove_dir_all(&work).await;
    result
}

async fn run(
    work: &Path,
    source: &[u8],
    is_video: bool,
) -> Result<TranscodeOutput, TranscodeError> {
    let input = work.join("input");
    tokio::fs::write(&input, source).await?;

    let manifest = work.join("index.m3u8");
    let seg_pattern = work.join("seg_%03d.ts");

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y").arg("-i").arg(&input);
    if is_video {
        cmd.args(["-c:v", "libx264", "-c:a", "aac"]);
    } else {
        cmd.args(["-vn", "-c:a", "aac"]);
    }
    cmd.args([
        "-hls_time",
        "4",
        "-hls_playlist_type",
        "vod",
        "-hls_segment_filename",
    ])
    .arg(&seg_pattern)
    .arg(&manifest);

    let out = cmd.output().await?;
    if !out.status.success() {
        return Err(TranscodeError::Ffmpeg(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ));
    }

    // Collect every produced file except the raw input.
    let mut files = Vec::new();
    let mut dir = tokio::fs::read_dir(work).await?;
    while let Some(entry) = dir.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "input" {
            continue;
        }
        let bytes = tokio::fs::read(entry.path()).await?;
        let content_type = if name.ends_with(".m3u8") {
            "application/vnd.apple.mpegurl"
        } else if name.ends_with(".ts") {
            "video/mp2t"
        } else {
            "application/octet-stream"
        };
        files.push(TranscodedFile {
            name,
            bytes,
            content_type,
        });
    }

    Ok(TranscodeOutput {
        files,
        manifest_name: "index.m3u8".to_string(),
    })
}
