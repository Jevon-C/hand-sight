//! Server settings: defaults, an optional TOML file, then environment overrides.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::jobs::HandModelChoice;

/// Settings that control where the server listens and where it keeps its files.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Address to listen on. Defaults to `127.0.0.1:8080`.
    pub bind: SocketAddr,
    /// Allow listening on an address other than this computer. Off by default, because the
    /// app has no sign-in and handles research video.
    pub allow_remote: bool,
    /// Folder that holds videos, results and the index.
    pub data_dir: PathBuf,
    /// Folder holding the built web app. Files are served from here at `/`.
    pub static_dir: PathBuf,
    /// Folder holding test plug-ins, one sub-folder each.
    pub plugins_dir: PathBuf,
    /// Python interpreter used to run the analysis tools (it must have `axonhand` installed).
    pub python: PathBuf,
    /// Largest video accepted, in bytes. Defaults to 2 GiB.
    pub max_upload_bytes: u64,
    /// Folder holding the hand model files (see `scripts/fetch_models.ps1`).
    pub models_dir: PathBuf,
    /// Which hand model to use. `auto` uses the real model, run by Python, when its files are
    /// present; `mediapipe_rust` runs it with the Rust tracker.
    pub hand_model: HandModelChoice,
    /// The Rust tracker program, used by `mediapipe_rust`. A bare name is looked for next to the
    /// server program and then on `PATH`.
    pub track_binary: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 8080)),
            allow_remote: false,
            data_dir: PathBuf::from("data"),
            static_dir: PathBuf::from("crates/ui/dist"),
            plugins_dir: PathBuf::from("plugins"),
            python: PathBuf::from("python"),
            max_upload_bytes: 2 * 1024 * 1024 * 1024,
            models_dir: PathBuf::from("models"),
            hand_model: HandModelChoice::Auto,
            track_binary: PathBuf::from("axonhand-track"),
        }
    }
}

/// Why the settings could not be used.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    /// The settings file could not be read.
    #[error("cannot read settings file {path}: {reason}")]
    Unreadable {
        /// The file that was tried.
        path: PathBuf,
        /// The system's explanation.
        reason: String,
    },
    /// The settings file is not valid TOML or has an unknown or mistyped setting.
    #[error("settings file {path} is invalid: {reason}")]
    Invalid {
        /// The file that was read.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// An environment variable held a value that could not be used.
    #[error("environment variable {name} is invalid: {reason}")]
    BadEnvironment {
        /// The variable's name.
        name: &'static str,
        /// What is wrong with the value.
        reason: String,
    },
    /// The address is reachable from other computers but remote access was not enabled.
    #[error("{0} is not a local address; set allow_remote to listen on it")]
    RemoteNotAllowed(IpAddr),
}

impl Config {
    /// Build settings from defaults, then the file at `file` if given, then the environment.
    ///
    /// Recognised variables: `AXONHAND_BIND`, `AXONHAND_ALLOW_REMOTE` (`true` or `false`),
    /// `AXONHAND_DATA_DIR`, `AXONHAND_STATIC_DIR`, `AXONHAND_PLUGINS_DIR`, `AXONHAND_PYTHON`, `AXONHAND_MODELS_DIR`, `AXONHAND_HAND_MODEL` (`auto`, `stand_in`, `mediapipe` or `mediapipe_rust`), `AXONHAND_TRACK`, `AXONHAND_MAX_UPLOAD_BYTES`. `env` returns a variable's value if set.
    ///
    /// # Errors
    ///
    /// A [`ConfigError`] if the file or a variable is unusable, or if the final address is not
    /// local and `allow_remote` is false.
    pub fn load(
        file: Option<&Path>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let mut config = match file {
            Some(path) => Self::from_file(path)?,
            None => Self::default(),
        };
        if let Some(value) = env("AXONHAND_BIND") {
            config.bind = value.parse().map_err(|e: std::net::AddrParseError| {
                ConfigError::BadEnvironment {
                    name: "AXONHAND_BIND",
                    reason: e.to_string(),
                }
            })?;
        }
        if let Some(value) = env("AXONHAND_ALLOW_REMOTE") {
            config.allow_remote = value.parse().map_err(|e: std::str::ParseBoolError| {
                ConfigError::BadEnvironment {
                    name: "AXONHAND_ALLOW_REMOTE",
                    reason: e.to_string(),
                }
            })?;
        }
        if let Some(value) = env("AXONHAND_DATA_DIR") {
            config.data_dir = PathBuf::from(value);
        }
        if let Some(value) = env("AXONHAND_STATIC_DIR") {
            config.static_dir = PathBuf::from(value);
        }
        if let Some(value) = env("AXONHAND_PLUGINS_DIR") {
            config.plugins_dir = PathBuf::from(value);
        }
        if let Some(value) = env("AXONHAND_PYTHON") {
            config.python = PathBuf::from(value);
        }
        if let Some(value) = env("AXONHAND_MODELS_DIR") {
            config.models_dir = PathBuf::from(value);
        }
        if let Some(value) = env("AXONHAND_TRACK") {
            config.track_binary = PathBuf::from(value);
        }
        if let Some(value) = env("AXONHAND_HAND_MODEL") {
            config.hand_model = match value.as_str() {
                "auto" => HandModelChoice::Auto,
                "stand_in" => HandModelChoice::StandIn,
                "mediapipe" => HandModelChoice::Mediapipe,
                "mediapipe_rust" => HandModelChoice::MediapipeRust,
                other => {
                    return Err(ConfigError::BadEnvironment {
                        name: "AXONHAND_HAND_MODEL",
                        reason: format!(
                            "{other:?} is not auto, stand_in, mediapipe or mediapipe_rust"
                        ),
                    })
                }
            };
        }
        if let Some(value) = env("AXONHAND_MAX_UPLOAD_BYTES") {
            config.max_upload_bytes = value.parse().map_err(|e: std::num::ParseIntError| {
                ConfigError::BadEnvironment {
                    name: "AXONHAND_MAX_UPLOAD_BYTES",
                    reason: e.to_string(),
                }
            })?;
        }
        config.check_bind()?;
        Ok(config)
    }

    fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Unreadable {
            path: path.to_owned(),
            reason: e.to_string(),
        })?;
        toml::from_str(&text).map_err(|e| ConfigError::Invalid {
            path: path.to_owned(),
            reason: e.message().to_owned(),
        })
    }

    fn check_bind(&self) -> Result<(), ConfigError> {
        let ip = self.bind.ip();
        if self.allow_remote || ip.is_loopback() {
            Ok(())
        } else {
            Err(ConfigError::RemoteNotAllowed(ip))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn the_hand_model_choice_defaults_to_auto_and_can_be_set() -> Result<(), ConfigError> {
        assert_eq!(
            Config::load(None, no_env)?.hand_model,
            HandModelChoice::Auto
        );
        let env = |name: &str| (name == "AXONHAND_HAND_MODEL").then(|| "stand_in".to_owned());
        assert_eq!(
            Config::load(None, env)?.hand_model,
            HandModelChoice::StandIn
        );
        let env = |name: &str| (name == "AXONHAND_MODELS_DIR").then(|| "m".to_owned());
        assert_eq!(Config::load(None, env)?.models_dir, PathBuf::from("m"));
        let env = |name: &str| (name == "AXONHAND_HAND_MODEL").then(|| "mediapipe_rust".to_owned());
        assert_eq!(
            Config::load(None, env)?.hand_model,
            HandModelChoice::MediapipeRust
        );
        assert_eq!(
            Config::load(None, no_env)?.track_binary,
            PathBuf::from("axonhand-track")
        );
        let env = |name: &str| (name == "AXONHAND_TRACK").then(|| "t".to_owned());
        assert_eq!(Config::load(None, env)?.track_binary, PathBuf::from("t"));
        Ok(())
    }

    #[test]
    fn an_unknown_hand_model_is_refused_by_name() {
        let env = |name: &str| (name == "AXONHAND_HAND_MODEL").then(|| "magic".to_owned());
        assert!(matches!(
            Config::load(None, env),
            Err(ConfigError::BadEnvironment {
                name: "AXONHAND_HAND_MODEL",
                ..
            })
        ));
    }

    #[test]
    fn defaults_listen_on_this_computer_only() -> Result<(), ConfigError> {
        let config = Config::load(None, no_env)?;
        assert_eq!(config.bind, SocketAddr::from(([127, 0, 0, 1], 8080)));
        assert!(!config.allow_remote);
        Ok(())
    }

    #[test]
    fn file_overrides_defaults_and_environment_overrides_file(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("axonhand.toml");
        std::fs::write(
            &path,
            "bind = \"127.0.0.1:9000\"\ndata_dir = \"from_file\"\n",
        )?;
        let env = |name: &str| (name == "AXONHAND_DATA_DIR").then(|| "from_env".to_owned());
        let config = Config::load(Some(&path), env)?;
        assert_eq!(config.bind.port(), 9000);
        assert_eq!(config.data_dir, PathBuf::from("from_env"));
        Ok(())
    }

    #[test]
    fn a_non_local_address_needs_explicit_permission() {
        let env = |name: &str| (name == "AXONHAND_BIND").then(|| "0.0.0.0:8080".to_owned());
        assert!(matches!(
            Config::load(None, env),
            Err(ConfigError::RemoteNotAllowed(_))
        ));
        let allowed = |name: &str| match name {
            "AXONHAND_BIND" => Some("0.0.0.0:8080".to_owned()),
            "AXONHAND_ALLOW_REMOTE" => Some("true".to_owned()),
            _ => None,
        };
        assert!(Config::load(None, allowed).is_ok());
    }

    #[test]
    fn bad_environment_values_are_named() {
        let env = |name: &str| (name == "AXONHAND_BIND").then(|| "not an address".to_owned());
        assert!(matches!(
            Config::load(None, env),
            Err(ConfigError::BadEnvironment {
                name: "AXONHAND_BIND",
                ..
            })
        ));
        let env = |name: &str| (name == "AXONHAND_ALLOW_REMOTE").then(|| "maybe".to_owned());
        assert!(matches!(
            Config::load(None, env),
            Err(ConfigError::BadEnvironment {
                name: "AXONHAND_ALLOW_REMOTE",
                ..
            })
        ));
    }

    #[test]
    fn python_and_upload_limit_can_be_set_from_the_environment() -> Result<(), ConfigError> {
        let env = |name: &str| match name {
            "AXONHAND_PYTHON" => Some("py312".to_owned()),
            "AXONHAND_MAX_UPLOAD_BYTES" => Some("1000".to_owned()),
            _ => None,
        };
        let config = Config::load(None, env)?;
        assert_eq!(config.python, PathBuf::from("py312"));
        assert_eq!(config.max_upload_bytes, 1000);
        let bad = |name: &str| (name == "AXONHAND_MAX_UPLOAD_BYTES").then(|| "lots".to_owned());
        assert!(matches!(
            Config::load(None, bad),
            Err(ConfigError::BadEnvironment { .. })
        ));
        Ok(())
    }

    #[test]
    fn unknown_settings_and_missing_files_are_errors() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("axonhand.toml");
        std::fs::write(&path, "colour = \"blue\"\n")?;
        assert!(matches!(
            Config::load(Some(&path), no_env),
            Err(ConfigError::Invalid { .. })
        ));
        assert!(matches!(
            Config::load(Some(&dir.path().join("absent.toml")), no_env),
            Err(ConfigError::Unreadable { .. })
        ));
        Ok(())
    }
}
