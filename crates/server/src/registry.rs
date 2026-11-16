//! Discovery of test plug-ins on disk.
//!
//! Each plug-in is a folder under the plug-ins directory holding a `plugin.toml`. The folder
//! name must equal the manifest's `id`, which keeps ids unique. A plug-in whose manifest is
//! missing, unreadable or invalid is still listed, with status `broken` and the reason, so a
//! mistake is visible in the app instead of the plug-in silently vanishing. One broken
//! plug-in never affects the others.

use std::path::{Path, PathBuf};

use axonhand_api::{CaptureRules, TestDetail, TestList, TestStatus, TestSummary};

use crate::manifest::{self, Manifest};

/// A plug-in that loaded.
#[derive(Debug, Clone, PartialEq)]
pub struct Plugin {
    /// Facts shown to clients.
    pub detail: TestDetail,
    /// Folder containing the plug-in's files.
    pub dir: PathBuf,
    /// Script to run, relative to `dir`. `None` for a planned test.
    pub entrypoint: Option<String>,
}

/// Everything found by one scan.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Registry {
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
enum Entry {
    Loaded(Plugin),
    Broken { detail: TestDetail },
}

impl Entry {
    fn detail(&self) -> &TestDetail {
        match self {
            Self::Loaded(plugin) => &plugin.detail,
            Self::Broken { detail } => detail,
        }
    }
}

fn broken(folder: &str, problems: &[String]) -> Entry {
    Entry::Broken {
        detail: TestDetail {
            summary: TestSummary {
                id: folder.to_owned(),
                version: String::new(),
                name: folder.to_owned(),
                clinical_item: String::new(),
                status: TestStatus::Broken,
                summary: "This plug-in could not be loaded.".to_owned(),
                error: Some(problems.join("; ")),
            },
            capture: CaptureRules {
                min_fps: 0.0,
                min_duration_seconds: 0.0,
                board_required: false,
                instructions: String::new(),
            },
            metrics: Vec::new(),
            parameters: Vec::new(),
        },
    }
}

fn load_one(dir: &Path, folder: &str) -> Entry {
    let manifest_path = dir.join("plugin.toml");
    let text = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => {
            let why = if error.kind() == std::io::ErrorKind::NotFound {
                "plugin.toml is missing".to_owned()
            } else {
                format!("plugin.toml cannot be read: {error}")
            };
            return broken(folder, &[why]);
        }
    };
    let Manifest { detail, entrypoint } = match manifest::parse(&text) {
        Ok(manifest) => manifest,
        Err(problems) => return broken(folder, &problems),
    };
    if detail.summary.id != folder {
        return broken(
            folder,
            &[format!(
                "the folder name must equal the manifest id ({:?})",
                detail.summary.id
            )],
        );
    }
    if let Some(script) = &entrypoint {
        if !dir.join(script).is_file() {
            return broken(folder, &[format!("entrypoint {script:?} does not exist")]);
        }
    }
    Entry::Loaded(Plugin {
        detail,
        dir: dir.to_owned(),
        entrypoint,
    })
}

impl Registry {
    /// Scan `plugins_dir`. A missing or unreadable directory gives an empty registry; the
    /// problem is logged.
    #[must_use]
    pub fn scan(plugins_dir: &Path) -> Self {
        let reader = match std::fs::read_dir(plugins_dir) {
            Ok(reader) => reader,
            Err(error) => {
                tracing::warn!(dir = %plugins_dir.display(), %error, "no plug-ins directory");
                return Self::default();
            }
        };
        let mut entries: Vec<Entry> = reader
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let folder = e.file_name().to_string_lossy().into_owned();
                (!folder.starts_with('.') && !folder.starts_with('_'))
                    .then(|| load_one(&e.path(), &folder))
            })
            .collect();
        entries.sort_by(|a, b| {
            let (a, b) = (&a.detail().summary, &b.detail().summary);
            a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id))
        });
        Self { entries }
    }

    /// Every plug-in found, ordered by name.
    #[must_use]
    pub fn list(&self) -> TestList {
        TestList {
            tests: self
                .entries
                .iter()
                .map(|e| e.detail().summary.clone())
                .collect(),
        }
    }

    /// Details of one test by id, including a broken one (with empty rules and metrics).
    #[must_use]
    pub fn detail(&self, id: &str) -> Option<&TestDetail> {
        self.entries
            .iter()
            .map(Entry::detail)
            .find(|d| d.summary.id == id)
    }

    /// A loaded, runnable plug-in by id. `None` for unknown, planned and broken ones.
    #[must_use]
    pub fn runnable(&self, id: &str) -> Option<&Plugin> {
        self.entries.iter().find_map(|entry| match entry {
            Entry::Loaded(p)
                if p.detail.summary.id == id
                    && p.detail.summary.status == TestStatus::Available =>
            {
                Some(p)
            }
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, status: &str, extra: &str) -> String {
        format!(
            r#"id = "{id}"
version = "1.0.0"
name = "Test {id}"
clinical_item = "X"
status = "{status}"
summary = "s"
{extra}
[capture]
min_fps = 50
min_duration_seconds = 5
instructions = "i"
"#
        )
    }

    fn plugin(root: &Path, folder: &str, text: &str, script: bool) -> std::io::Result<()> {
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("plugin.toml"), text)?;
        if script {
            std::fs::write(dir.join("p.py"), "")?;
        }
        Ok(())
    }

    #[test]
    fn good_planned_and_broken_plugins_are_all_listed() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        plugin(
            dir.path(),
            "alpha",
            &manifest("alpha", "available", "entrypoint = \"p.py\""),
            true,
        )?;
        plugin(dir.path(), "beta", &manifest("beta", "planned", ""), false)?;
        plugin(dir.path(), "gamma", "this is not toml", false)?;
        let registry = Registry::scan(dir.path());
        let list = registry.list().tests;
        let status = |id: &str| list.iter().find(|t| t.id == id).map(|t| t.status);
        assert_eq!(status("alpha"), Some(TestStatus::Available));
        assert_eq!(status("beta"), Some(TestStatus::Planned));
        assert_eq!(status("gamma"), Some(TestStatus::Broken));
        assert!(list
            .iter()
            .find(|t| t.id == "gamma")
            .and_then(|t| t.error.clone())
            .is_some());
        assert!(registry.runnable("alpha").is_some());
        assert!(
            registry.runnable("beta").is_none(),
            "planned tests cannot run"
        );
        assert!(
            registry.runnable("gamma").is_none(),
            "broken tests cannot run"
        );
        assert!(registry.runnable("zzz").is_none());
        Ok(())
    }

    #[test]
    fn a_broken_plugin_does_not_hide_the_others() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir_all(dir.path().join("empty"))?;
        plugin(
            dir.path(),
            "ok",
            &manifest("ok", "available", "entrypoint = \"p.py\""),
            true,
        )?;
        let registry = Registry::scan(dir.path());
        assert_eq!(registry.list().tests.len(), 2);
        let empty = registry.detail("empty").map(|d| d.summary.error.clone());
        assert_eq!(empty, Some(Some("plugin.toml is missing".to_owned())));
        assert!(registry.runnable("ok").is_some());
        Ok(())
    }

    #[test]
    fn folder_name_must_match_the_id() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        plugin(
            dir.path(),
            "wrong",
            &manifest("right", "planned", ""),
            false,
        )?;
        let registry = Registry::scan(dir.path());
        let error = registry
            .detail("wrong")
            .and_then(|d| d.summary.error.clone());
        assert!(error.is_some_and(|e| e.contains("folder name")));
        Ok(())
    }

    #[test]
    fn a_missing_entrypoint_file_is_an_error() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        plugin(
            dir.path(),
            "a",
            &manifest("a", "available", "entrypoint = \"p.py\""),
            false,
        )?;
        let registry = Registry::scan(dir.path());
        let error = registry.detail("a").and_then(|d| d.summary.error.clone());
        assert!(error.is_some_and(|e| e.contains("does not exist")));
        Ok(())
    }

    #[test]
    fn hidden_folders_and_files_are_ignored_and_a_missing_directory_is_empty(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir_all(dir.path().join(".git"))?;
        std::fs::create_dir_all(dir.path().join("_template"))?;
        std::fs::write(dir.path().join("README.md"), "x")?;
        assert!(Registry::scan(dir.path()).list().tests.is_empty());
        assert!(Registry::scan(&dir.path().join("absent"))
            .list()
            .tests
            .is_empty());
        Ok(())
    }

    #[test]
    fn the_list_is_ordered_by_name() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        for id in ["zeta", "alpha", "mid"] {
            plugin(dir.path(), id, &manifest(id, "planned", ""), false)?;
        }
        let ids: Vec<String> = Registry::scan(dir.path())
            .list()
            .tests
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["alpha", "mid", "zeta"]);
        Ok(())
    }
}
