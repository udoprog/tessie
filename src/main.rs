use failure::{bail, format_err};
use std::{
    path::{Path, PathBuf},
    process,
};

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

/// The format to transcode to.
pub enum Format {
    /// YouTube-optimized format (1080p @ 60fps)
    YouTube,
    /// High-quality GIF.
    Gif,
    /// Copy input parameters.
    Copy,
}

impl Format {
    pub fn input_args(&self, cmd: &mut process::Command) {
        use self::Format::*;

        match *self {
            YouTube => {
                cmd.args(&["-y", "-hwaccel", "cuvid", "-c:v", "h264_cuvid"]);
            }
            _ => {}
        }
    }

    /// Construct an output file based on the input.
    pub fn output_file(&self, input: &Path) -> Result<PathBuf, failure::Error> {
        use self::Format::*;

        let mut output = input.to_owned();

        match *self {
            YouTube => {
                output.set_extension("mp4");
            }
            Gif => {
                output.set_extension("gif");
            }
            Copy => {
                let e = match output.extension().and_then(|s| s.to_str()) {
                    Some(ext) => ext.to_string(),
                    None => failure::bail!("expected extension: {}", output.display()),
                };

                output.set_extension(format!("copy.{}", e));
            }
        }

        Ok(output)
    }

    pub fn output_args(&self, cmd: &mut process::Command) {
        use self::Format::*;

        match *self {
            YouTube => {
                cmd.args(&[
                    "-c:v",
                    "h264_nvenc",
                    "-coder",
                    "1",
                    "-preset",
                    "llhq",
                    "-rc:v",
                    "vbr_minqp",
                    "-qmin:v",
                    "21",
                    "-qmax:v",
                    "23",
                    "-b:v",
                    "5000k",
                    "-maxrate:v",
                    "8000k",
                    "-profile:v",
                    "high",
                    "-bf",
                    "2",
                    "-c:a",
                    "aac",
                    "-profile:a",
                    "aac_low",
                    "-b:a",
                    "384k",
                    "-f",
                    "mp4",
                ]);
            }
            Gif => {
                cmd.args(&[
                    "-filter_complex",
                    "[0:v] fps=12,scale=280:-1,split [a][b];[a] palettegen [p];[b][p] paletteuse",
                    "-f",
                    "gif",
                ]);
            }
            Copy => {
                cmd.args(&["-c:v", "copy", "-c:a", "copy"]);
            }
        }
    }
}

/// ffmpeg abstraction.
#[derive(Default)]
struct Ffmpeg {
    map: Vec<String>,
    start: Option<String>,
    end: Option<String>,
    duration: Option<String>,
}

impl Ffmpeg {
    const COMMAND: &'static str = "ffmpeg";

    /// Create a new ffmpeg abstraction testing that we have a workable command in the process.
    pub fn new() -> Result<Ffmpeg, failure::Error> {
        let o = process::Command::new(Self::COMMAND)
            .arg("-version")
            .output()?;

        if !o.status.success() {
            bail!("could not run: ffmpeg --version`: {:?}", o);
        }

        Ok(Ffmpeg::default())
    }

    /// Transcode a single file from input to output.
    pub fn transcode(
        &self,
        format: Format,
        input: impl AsRef<Path>,
        output: impl AsRef<Path>,
    ) -> Result<(), failure::Error> {
        let mut cmd = process::Command::new(Self::COMMAND);

        if let Some(start) = self.start.as_ref() {
            cmd.args(&["-ss", start.as_str()]);
        }

        if let Some(end) = self.end.as_ref() {
            cmd.args(&["-to", end.as_str()]);
        }

        if let Some(duration) = self.duration.as_ref() {
            cmd.args(&["-t", duration.as_str()]);
        }

        format.input_args(&mut cmd);
        cmd.arg("-i");
        cmd.arg(input.as_ref());

        for m in &self.map {
            cmd.arg("-map");
            cmd.arg(m);
        }

        format.output_args(&mut cmd);
        cmd.arg(output.as_ref());

        println!("{:?}", cmd);

        if !cmd.status()?.success() {
            bail!("failed to run command");
        }

        Ok(())
    }
}

fn opts() -> clap::App<'static, 'static> {
    clap::App::new("tessie")
        .version(VERSION)
        .author("John-John Tedro <udoprog@tedro.se>")
        .about("Transcodes videos using ffmpeg into different formats.")
        .arg(
            clap::Arg::with_name("input")
                .help("Input file to transcode.")
                .required(true),
        )
        .arg(
            clap::Arg::with_name("format")
                .help(
                    "The format of the transcode (default: YouTube). Available formats: YouTube, Gif.",
                )
                .short("f")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("start")
                .help("At which timestamp we should transcode from.")
                .short("s")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("end")
                .help("At which timestamp the transcoding should end.")
                .short("e")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("map")
                .short("m")
                .help("Map tracks (0:0 is usually video, 0:1=first audio).")
                .multiple(true)
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("duration")
                .help("How long the transcoding should be.")
                .short("d")
                .takes_value(true),
        )
}

fn main() -> Result<(), failure::Error> {
    let m = opts().get_matches();

    let mut ffmpeg = Ffmpeg::new()?;

    let format = match m.value_of("format") {
        None | Some("youtube") | Some("YouTube") => Format::YouTube,
        Some("gif") | Some("Gif") => Format::Gif,
        Some("copy") | Some("Copy") => Format::Copy,
        Some(other) => bail!("illegal --format: {}", other),
    };

    ffmpeg.map = m
        .values_of("map")
        .map(|o| o.map(|s| s.to_string()).collect())
        .unwrap_or_default();
    ffmpeg.start = m.value_of("start").map(String::from);
    ffmpeg.end = m.value_of("end").map(String::from);
    ffmpeg.duration = m.value_of("duration").map(String::from);

    let input = m
        .value_of("input")
        .map(PathBuf::from)
        .ok_or_else(|| format_err!("missing <input> argument"))?;
    let output = format.output_file(&input)?;

    if output.is_file() {
        bail!("output already exists: {}", output.display());
    }

    ffmpeg.transcode(format, &input, &output)?;
    Ok(())
}
