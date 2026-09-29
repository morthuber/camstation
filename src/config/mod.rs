//! Configuration models, validation, and atomic persistence.

use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SCHEMA_VERSION: u32 = 1;
const MAX_CAMERAS_PER_VIEW: usize = 10;
pub(crate) const MAX_GRID_EXTENT: u32 = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AppConfig {
    pub(crate) schema_version: u32,
    pub(crate) cameras: Vec<CameraConfig>,
    pub(crate) views: Vec<ViewConfig>,
    pub(crate) startup_view: Option<Uuid>,
    pub(crate) kiosk_on_start: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CameraConfig {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) rtsp_url: String,
    pub(crate) substream_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ViewConfig {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    #[serde(default = "default_grid_extent")]
    pub(crate) columns: u32,
    #[serde(default = "default_grid_extent")]
    pub(crate) rows: u32,
    pub(crate) tiles: Vec<ViewTile>,
}

fn default_grid_extent() -> u32 {
    4
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ViewTile {
    pub(crate) camera_id: Uuid,
    pub(crate) column: u32,
    pub(crate) row: u32,
    pub(crate) column_span: u32,
    pub(crate) row_span: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        let overview_id = Uuid::new_v4();
        Self {
            schema_version: SCHEMA_VERSION,
            cameras: Vec::new(),
            views: vec![ViewConfig {
                id: overview_id,
                name: "Overview".to_owned(),
                columns: 1,
                rows: 1,
                tiles: Vec::new(),
            }],
            startup_view: Some(overview_id),
            kiosk_on_start: false,
        }
    }
}

impl AppConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported configuration schema version {}; expected {SCHEMA_VERSION}",
                self.schema_version
            );
        }

        let mut camera_ids = HashSet::with_capacity(self.cameras.len());
        for camera in &self.cameras {
            if !camera_ids.insert(camera.id) {
                bail!("duplicate camera ID {}", camera.id);
            }
            require_nonblank("camera", camera.id, &camera.name)?;
            validate_rtsp_url(&camera.rtsp_url)
                .with_context(|| format!("camera '{}' has an invalid RTSP URL", camera.name))?;
            if let Some(url) = &camera.substream_url {
                validate_rtsp_url(url).with_context(|| {
                    format!("camera '{}' has an invalid substream URL", camera.name)
                })?;
            }
        }

        if self.views.is_empty() {
            bail!("configuration must contain at least one view");
        }

        let mut view_ids = HashSet::with_capacity(self.views.len());
        let mut view_names = HashSet::with_capacity(self.views.len());
        for view in &self.views {
            if !view_ids.insert(view.id) {
                bail!("duplicate view ID {}", view.id);
            }
            require_nonblank("view", view.id, &view.name)?;
            if !view_names.insert(view.name.trim().to_lowercase()) {
                bail!("duplicate view name '{}'", view.name);
            }
            if view.columns == 0 || view.rows == 0 {
                bail!("view '{}' must have at least one row and column", view.name);
            }
            if view.columns > MAX_GRID_EXTENT || view.rows > MAX_GRID_EXTENT {
                bail!(
                    "view '{}' exceeds the maximum grid size of {MAX_GRID_EXTENT} by {MAX_GRID_EXTENT}",
                    view.name
                );
            }
            if view.tiles.len() > MAX_CAMERAS_PER_VIEW {
                bail!(
                    "view '{}' contains {} cameras; at most {MAX_CAMERAS_PER_VIEW} are allowed",
                    view.name,
                    view.tiles.len()
                );
            }

            let mut tiled_camera_ids = HashSet::with_capacity(view.tiles.len());
            for (index, tile) in view.tiles.iter().enumerate() {
                if !camera_ids.contains(&tile.camera_id) {
                    bail!(
                        "view '{}' references missing camera {}",
                        view.name,
                        tile.camera_id
                    );
                }
                if !tiled_camera_ids.insert(tile.camera_id) {
                    bail!(
                        "view '{}' contains camera {} more than once",
                        view.name,
                        tile.camera_id
                    );
                }
                if tile.column_span == 0 || tile.row_span == 0 {
                    bail!(
                        "view '{}' has a tile for camera {} with a zero span",
                        view.name,
                        tile.camera_id
                    );
                }
                let Some(column_end) = tile.column.checked_add(tile.column_span) else {
                    bail!(
                        "view '{}' has a tile for camera {} whose coordinates overflow",
                        view.name,
                        tile.camera_id
                    );
                };
                let Some(row_end) = tile.row.checked_add(tile.row_span) else {
                    bail!(
                        "view '{}' has a tile for camera {} whose coordinates overflow",
                        view.name,
                        tile.camera_id
                    );
                };
                if column_end > view.columns || row_end > view.rows {
                    bail!(
                        "view '{}' has a tile for camera {} outside its {} by {} grid",
                        view.name,
                        tile.camera_id,
                        view.columns,
                        view.rows
                    );
                }
                if view.tiles[..index]
                    .iter()
                    .any(|other| tiles_overlap(tile, other))
                {
                    bail!(
                        "view '{}' has overlapping tiles involving camera {}",
                        view.name,
                        tile.camera_id
                    );
                }
            }
        }

        if let Some(startup_view) = self.startup_view
            && !view_ids.contains(&startup_view)
        {
            bail!("startup view {startup_view} does not exist");
        }

        Ok(())
    }
}

fn tiles_overlap(left: &ViewTile, right: &ViewTile) -> bool {
    left.column < right.column + right.column_span
        && right.column < left.column + left.column_span
        && left.row < right.row + right.row_span
        && right.row < left.row + left.row_span
}

fn require_nonblank(kind: &str, id: Uuid, name: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("{kind} {id} has a blank name");
    }
    Ok(())
}

fn validate_rtsp_url(url: &str) -> Result<()> {
    if url.trim() != url || url.chars().any(char::is_whitespace) {
        bail!("URL must not contain whitespace");
    }

    let scheme_length = if url
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("rtsp://"))
    {
        7
    } else if url
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("rtsps://"))
    {
        8
    } else {
        bail!("URL must use the rtsp:// or rtsps:// scheme");
    };
    let remainder = &url[scheme_length..];
    let authority = remainder.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if host.is_empty() || host == ":" {
        bail!("URL must include a host");
    }

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub(crate) fn from_override(path: Option<&Path>) -> Result<Self> {
        if let Some(path) = path {
            return Ok(Self {
                path: path.to_owned(),
            });
        }

        let base = nonempty_env("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| nonempty_env("HOME").map(|home| PathBuf::from(home).join(".config")))
            .context(
                "cannot determine configuration directory: neither XDG_CONFIG_HOME nor HOME is set",
            )?;

        Ok(Self {
            path: base.join("camstation").join("config.json"),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn load(&self) -> Result<AppConfig> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(AppConfig::default());
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to read configuration {}", self.path.display())
                });
            }
        };

        parse_and_validate(&bytes, &self.path, "loaded configuration")
    }

    pub(crate) fn save(&self, config: &AppConfig) -> Result<()> {
        config
            .validate()
            .context("refusing to save invalid configuration")?;

        let parent = usable_parent(&self.path);
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create configuration directory {}",
                parent.display()
            )
        })?;

        let existing = match fs::read(&self.path) {
            Ok(bytes) => {
                parse_and_validate(&bytes, &self.path, "existing configuration").context(
                    "refusing to overwrite the existing configuration; fix or move it first",
                )?;
                Some(bytes)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect existing configuration {}",
                        self.path.display()
                    )
                });
            }
        };

        let mut serialized = serde_json::to_vec_pretty(config)
            .context("failed to serialize configuration as JSON")?;
        serialized.push(b'\n');
        let config_temp = write_temporary(&self.path, "tmp", &serialized)?;

        if let Some(existing) = existing {
            let backup_path = appended_path(&self.path, ".bak");
            let backup_temp = match write_temporary(&backup_path, "tmp", &existing) {
                Ok(path) => path,
                Err(error) => {
                    let _ = fs::remove_file(&config_temp);
                    return Err(error);
                }
            };
            if let Err(error) = fs::rename(&backup_temp, &backup_path) {
                let _ = fs::remove_file(&backup_temp);
                let _ = fs::remove_file(&config_temp);
                return Err(error).with_context(|| {
                    format!(
                        "failed to replace configuration backup {}",
                        backup_path.display()
                    )
                });
            }
            if let Err(error) = sync_parent(parent) {
                let _ = fs::remove_file(&config_temp);
                return Err(error);
            }
        }

        if let Err(error) = fs::rename(&config_temp, &self.path) {
            let _ = fs::remove_file(&config_temp);
            return Err(error)
                .with_context(|| format!("failed to atomically replace {}", self.path.display()));
        }
        sync_parent(parent)?;

        Ok(())
    }
}

fn nonempty_env(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

fn usable_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn parse_and_validate(bytes: &[u8], path: &Path, description: &str) -> Result<AppConfig> {
    let config: AppConfig = serde_json::from_slice(bytes)
        .with_context(|| format!("failed to parse {description} at {}", path.display()))?;
    config
        .validate()
        .with_context(|| format!("{description} at {} is invalid", path.display()))?;
    Ok(config)
}

fn appended_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

fn write_temporary(target: &Path, label: &str, contents: &[u8]) -> Result<PathBuf> {
    let parent = usable_parent(target);
    let file_name = target
        .file_name()
        .context("configuration path must have a file name")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{file_name}.{label}.{}", Uuid::new_v4()));

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let result = (|| -> Result<()> {
        let mut file = options
            .open(&temporary)
            .with_context(|| format!("failed to create temporary file {}", temporary.display()))?;
        file.write_all(contents)
            .with_context(|| format!("failed to write temporary file {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temporary file {}", temporary.display()))?;
        Ok(())
    })();

    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    Ok(temporary)
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<()> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("failed to sync directory {}", parent.display()))
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = env::temp_dir().join(format!("camstation-config-test-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn config_path(&self) -> PathBuf {
            self.0.join("nested").join("config.json")
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn camera(name: &str) -> CameraConfig {
        CameraConfig {
            id: Uuid::new_v4(),
            name: name.to_owned(),
            rtsp_url: "rtsp://camera.example/live".to_owned(),
            substream_url: Some("rtsps://camera.example/low".to_owned()),
        }
    }

    #[test]
    fn default_has_selected_empty_overview() {
        let config = AppConfig::default();

        assert_eq!(config.schema_version, SCHEMA_VERSION);
        assert!(config.cameras.is_empty());
        assert_eq!(config.views.len(), 1);
        assert_eq!(config.views[0].name, "Overview");
        assert_eq!((config.views[0].columns, config.views[0].rows), (1, 1));
        assert!(config.views[0].tiles.is_empty());
        assert_eq!(config.startup_view, Some(config.views[0].id));
        assert!(!config.kiosk_on_start);
        config.validate().expect("default config should be valid");
    }

    #[test]
    fn round_trips_through_store() {
        let directory = TestDirectory::new();
        let store = ConfigStore::from_override(Some(&directory.config_path())).unwrap();
        let mut config = AppConfig::default();
        let camera = camera("Front door");
        config.views[0].columns = 2;
        config.views[0].tiles.push(ViewTile {
            camera_id: camera.id,
            column: 0,
            row: 0,
            column_span: 2,
            row_span: 1,
        });
        config.cameras.push(camera);
        config.kiosk_on_start = true;

        store.save(&config).expect("save config");

        assert_eq!(store.path(), directory.config_path());
        assert_eq!(store.load().expect("load config"), config);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn load_returns_default_when_file_is_absent() {
        let directory = TestDirectory::new();
        let store = ConfigStore::from_override(Some(&directory.config_path())).unwrap();

        let config = store.load().expect("load absent config");

        assert_eq!(config.views.len(), 1);
        assert_eq!(config.startup_view, Some(config.views[0].id));
    }

    #[test]
    fn validation_rejects_invalid_semantics() {
        let config = AppConfig {
            schema_version: 2,
            ..AppConfig::default()
        };
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("schema")
        );

        let mut config = AppConfig::default();
        config.views[0].name = "  ".to_owned();
        assert!(config.validate().unwrap_err().to_string().contains("blank"));

        let mut config = AppConfig::default();
        config.cameras.push(CameraConfig {
            id: Uuid::new_v4(),
            name: "Bad URL".to_owned(),
            rtsp_url: "https://camera.example/live".to_owned(),
            substream_url: None,
        });
        assert!(config.validate().unwrap_err().to_string().contains("RTSP"));

        let mut config = AppConfig::default();
        config.views[0].tiles.push(ViewTile {
            camera_id: Uuid::new_v4(),
            column: 0,
            row: 0,
            column_span: 1,
            row_span: 1,
        });
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("missing camera")
        );

        let config = AppConfig {
            startup_view: Some(Uuid::new_v4()),
            ..AppConfig::default()
        };
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("startup view")
        );

        let config = AppConfig {
            views: Vec::new(),
            startup_view: None,
            ..AppConfig::default()
        };
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("at least one view")
        );
    }

    #[test]
    fn validation_rejects_invalid_grid_geometry() {
        let mut config = AppConfig::default();
        let first = camera("First");
        let second = camera("Second");
        config.cameras.extend([first.clone(), second.clone()]);
        config.views[0].columns = 2;
        config.views[0].rows = 2;
        config.views[0].tiles = vec![
            ViewTile {
                camera_id: first.id,
                column: 0,
                row: 0,
                column_span: 2,
                row_span: 1,
            },
            ViewTile {
                camera_id: second.id,
                column: 1,
                row: 0,
                column_span: 1,
                row_span: 1,
            },
        ];
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("overlapping")
        );

        config.views[0].tiles.pop();
        config.views[0].tiles[0].column = 1;
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("outside")
        );
    }

    #[test]
    fn validation_rejects_ambiguous_view_names() {
        let mut config = AppConfig::default();
        let mut duplicate = config.views[0].clone();
        duplicate.id = Uuid::new_v4();
        duplicate.name = " overview ".to_owned();
        config.views.push(duplicate);

        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("duplicate view name")
        );
    }

    #[test]
    fn unknown_fields_and_pre_dimension_m3_files_are_accepted() {
        let view_id = Uuid::new_v4();
        let json = format!(
            r#"{{
                "schema_version": 1,
                "cameras": [],
                "views": [{{
                    "id": "{view_id}",
                    "name": "Overview",
                    "tiles": [],
                    "future_view_option": true
                }}],
                "startup_view": "{view_id}",
                "kiosk_on_start": false,
                "future_root_option": "ignored"
            }}"#
        );

        let config: AppConfig = serde_json::from_str(&json).expect("deserialize config");

        assert_eq!((config.views[0].columns, config.views[0].rows), (4, 4));
        config.validate().expect("validate compatible config");
    }

    #[test]
    fn atomic_save_retains_previous_config_as_backup() {
        let directory = TestDirectory::new();
        let store = ConfigStore::from_override(Some(&directory.config_path())).unwrap();
        let first = AppConfig::default();
        store.save(&first).expect("first save");

        let mut second = first.clone();
        second.kiosk_on_start = true;
        store.save(&second).expect("second save");

        assert_eq!(store.load().unwrap(), second);
        let backup_path = appended_path(store.path(), ".bak");
        let backup = parse_and_validate(
            &fs::read(&backup_path).expect("read backup"),
            &backup_path,
            "backup",
        )
        .expect("valid backup");
        assert_eq!(backup, first);
        let entries: Vec<_> = fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(
            !entries
                .iter()
                .any(|name| name.to_string_lossy().contains(".tmp."))
        );
    }

    #[test]
    fn malformed_existing_config_is_not_overwritten() {
        let directory = TestDirectory::new();
        let path = directory.config_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let malformed = b"{ definitely not json";
        fs::write(&path, malformed).unwrap();
        let store = ConfigStore::from_override(Some(&path)).unwrap();

        let error = store.save(&AppConfig::default()).unwrap_err();

        assert!(error.to_string().contains("refusing to overwrite"));
        assert_eq!(fs::read(path).unwrap(), malformed);
    }
}
