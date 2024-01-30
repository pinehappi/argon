use rbx_dom_weak::types::Variant;
use serde::{Deserialize, Serialize};
use std::{
	collections::HashMap,
	path::{Path, PathBuf},
};

use crate::{glob::Glob, middleware::FileType, project::Project, util};

#[derive(Debug, Clone)]
pub struct ResolvedSyncRule {
	pub file_type: FileType,
	pub path: PathBuf,
	pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SyncRule {
	#[serde(rename = "type")]
	pub file_type: FileType,

	pub pattern: Option<Glob>,
	pub child_pattern: Option<Glob>,
	pub exclude: Option<Glob>,

	pub suffix: Option<String>,
}

impl SyncRule {
	pub fn matches(&self, path: &Path) -> bool {
		if let Some(pattern) = &self.pattern {
			if pattern.matches_path(path) {
				return !self.is_excluded(path);
			}
		}

		false
	}

	pub fn matches_child(&self, path: &Path) -> bool {
		if let Some(child_pattern) = &self.child_pattern {
			let path = path.join(child_pattern.as_str());

			if child_pattern.matches_path(&path) {
				return !self.is_excluded(&path);
			}
		}

		false
	}

	pub fn is_excluded(&self, path: &Path) -> bool {
		self.exclude
			.as_ref()
			.map(|exclude| exclude.matches_path(path))
			.unwrap_or(false)
	}

	pub fn get_name(&self, path: &Path) -> String {
		if let Some(suffix) = &self.suffix {
			let name = util::get_file_name(path);
			name.strip_suffix(suffix).unwrap_or(name).into()
		} else {
			util::get_file_stem(path).into()
		}
	}

	pub fn resolve(&self, path: &Path) -> Option<ResolvedSyncRule> {
		if let Some(pattern) = &self.pattern {
			if pattern.matches_path(path) && !self.is_excluded(path) {
				return Some(ResolvedSyncRule {
					file_type: self.file_type.clone(),
					path: path.to_path_buf(),
					name: self.get_name(path),
				});
			}
		}

		None
	}

	pub fn resolve_child(&self, path: &Path) -> Option<ResolvedSyncRule> {
		if let Some(child_pattern) = &self.child_pattern {
			let path = path.join(child_pattern.as_str());
			let child_pattern = Glob::from_path(&path).unwrap();

			if let Some(path) = child_pattern.first() {
				if self.is_excluded(&path) {
					return None;
				}

				let name = util::get_file_name(path.parent().unwrap());

				return Some(ResolvedSyncRule {
					file_type: self.file_type.clone(),
					name: name.to_string(),
					path,
				});
			}
		}

		None
	}
}

#[derive(Debug, Clone)]
pub struct ProjectData {
	pub name: String,
	pub applies_to: PathBuf,
	pub class: Option<String>,
	pub properties: Option<HashMap<String, Variant>>,
}

impl ProjectData {
	pub fn new(name: &str, applies_to: &Path) -> Self {
		Self {
			name: name.to_string(),
			applies_to: applies_to.to_path_buf(),
			class: None,
			properties: None,
		}
	}

	pub fn set_class(&mut self, class: String) {
		self.class = Some(class);
	}

	pub fn set_properties(&mut self, properties: HashMap<String, Variant>) {
		self.properties = Some(properties);
	}
}

#[derive(Debug, Clone)]
pub struct Meta {
	pub sync_rules: Vec<SyncRule>,
	pub ignore_globs: Vec<Glob>,
	pub project_data: Option<ProjectData>,
}

impl Meta {
	// Creating new meta

	pub fn new() -> Self {
		Self {
			sync_rules: Vec::new(),
			ignore_globs: Vec::new(),
			project_data: None,
		}
	}

	pub fn from_project(project: &Project) -> Self {
		Self {
			sync_rules: project.sync_rules.clone().unwrap_or_else(|| Meta::default().sync_rules),
			ignore_globs: project.ignore_globs.clone().unwrap_or_default(),
			project_data: None,
		}
	}

	pub fn with_sync_rules(mut self, sync_rules: Vec<SyncRule>) -> Self {
		self.sync_rules = sync_rules;
		self
	}

	pub fn with_ignore_globs(mut self, ignore_globs: Vec<Glob>) -> Self {
		self.ignore_globs = ignore_globs;
		self
	}

	pub fn with_project_data(mut self, project_data: ProjectData) -> Self {
		self.project_data = Some(project_data);
		self
	}

	// Overwriting meta fields

	pub fn set_sync_rules(&mut self, sync_rules: Vec<SyncRule>) {
		self.sync_rules = sync_rules;
	}

	pub fn set_ignore_globs(&mut self, ignore_globs: Vec<Glob>) {
		self.ignore_globs = ignore_globs;
	}

	pub fn set_project_data(&mut self, project_data: ProjectData) {
		self.project_data = Some(project_data);
	}

	// Adding to meta fields

	pub fn add_sync_rule(&mut self, sync_rule: SyncRule) {
		self.sync_rules.push(sync_rule);
	}

	pub fn add_ignore_glob(&mut self, ignore_glob: Glob) {
		self.ignore_globs.push(ignore_glob);
	}

	// Joining meta fields

	pub fn extend_sync_rules(&mut self, sync_rules: Vec<SyncRule>) {
		self.sync_rules.extend(sync_rules);
	}

	pub fn extend_ignore_globs(&mut self, ignore_globs: Vec<Glob>) {
		self.ignore_globs.extend(ignore_globs);
	}

	pub fn extend(&mut self, meta: Meta) {
		self.extend_sync_rules(meta.sync_rules);
		self.extend_ignore_globs(meta.ignore_globs);

		if let Some(project_data) = meta.project_data {
			self.project_data = Some(project_data);
		}
	}

	// Misc

	pub fn is_empty(&self) -> bool {
		self.sync_rules.is_empty() && self.ignore_globs.is_empty() && self.project_data.is_none()
	}
}

macro_rules! sync_rule {
	($pattern:expr, $child_pattern:expr, $file_type:ident) => {
		SyncRule {
			file_type: FileType::$file_type,

			pattern: Some(Glob::new($pattern).unwrap()),
			child_pattern: Some(Glob::new($child_pattern).unwrap()),
			exclude: None,

			suffix: None,
		}
	};
	($pattern:expr, $child_pattern:expr, $file_type:ident, $suffix:expr) => {
		SyncRule {
			file_type: FileType::$file_type,

			pattern: Some(Glob::new($pattern).unwrap()),
			child_pattern: Some(Glob::new($child_pattern).unwrap()),
			exclude: None,

			suffix: Some($suffix.to_string()),
		}
	};
	($pattern:expr, $child_pattern:expr, $file_type:ident, $suffix:expr, $exclude:expr) => {
		SyncRule {
			file_type: FileType::$file_type,

			pattern: Some(Glob::new($pattern).unwrap()),
			child_pattern: Some(Glob::new($child_pattern).unwrap()),
			exclude: Some(Glob::new($exclude).unwrap()),

			suffix: Some($suffix.to_string()),
		}
	};
	($child_pattern:expr, $file_type:ident) => {
		SyncRule {
			file_type: FileType::$file_type,

			pattern: None,
			child_pattern: Some(Glob::new($child_pattern).unwrap()),
			exclude: None,

			suffix: None,
		}
	};
}

impl Default for Meta {
	fn default() -> Self {
		let sync_rules = vec![
			sync_rule!("*.project.json", Project),
			sync_rule!(".data.json", InstanceData),
			//
			sync_rule!("*.server.lua", ".src.server.lua", ServerScript, ".server.lua"),
			sync_rule!("*.client.lua", ".src.client.lua", ClientScript, ".client.lua"),
			sync_rule!("*.lua", ".src.lua", ModuleScript),
			sync_rule!("*.server.luau", ".src.server.luau", ServerScript, ".server.luau"),
			sync_rule!("*.client.luau", ".src.client.luau", ClientScript, ".client.luau"),
			sync_rule!("*.luau", ".src.luau", ModuleScript),
			//
			sync_rule!("*.txt", ".src.txt", StringValue),
			sync_rule!("*.csv", ".src.csv", LocalizationTable),
			sync_rule!("*.json", ".src.json", JsonModule, ".data.json"),
			sync_rule!("*.toml", ".src.toml", TomlModule),
			sync_rule!("*.rbxm", ".src.rbxm", RbxmModel),
			sync_rule!("*.rbxmx", ".src.rbxmx", RbxmxModel),
		];

		Self {
			sync_rules,
			ignore_globs: vec![],
			project_data: None,
		}
	}
}
