use rbx_dom_weak::types::{Ref, Variant};
use std::{
	collections::HashMap,
	fmt::Debug,
	path::{Path, PathBuf},
};

use super::meta::Meta;
use crate::middleware::FileType;

pub struct Snapshot {
	// For change processing
	pub id: Option<Ref>,
	pub file_type: Option<FileType>,

	// For middleware
	pub meta: Option<Meta>,
	pub path: Option<PathBuf>,

	// Roblox related
	pub name: String,
	pub class: String,
	pub properties: HashMap<String, Variant>,
	pub children: Vec<Snapshot>,
}

impl Snapshot {
	// Creating new snapshot

	pub fn new(name: &str) -> Self {
		Self {
			id: None,
			file_type: None,
			meta: None,
			path: None,
			name: name.to_string(),
			class: String::from("Folder"),
			properties: HashMap::new(),
			children: Vec::new(),
		}
	}

	pub fn with_id(mut self, id: Ref) -> Self {
		self.id = Some(id);
		self
	}

	pub fn with_file_type(mut self, file_type: FileType) -> Self {
		self.file_type = Some(file_type);
		self
	}

	pub fn with_meta(mut self, meta: Meta) -> Self {
		self.meta = Some(meta);
		self
	}

	pub fn with_path(mut self, path: &Path) -> Self {
		self.path = Some(path.into());
		self
	}

	pub fn with_name(mut self, name: &str) -> Self {
		self.name = name.into();
		self
	}

	pub fn with_class(mut self, class: &str) -> Self {
		self.class = class.into();
		self
	}

	pub fn with_properties(mut self, properties: HashMap<String, Variant>) -> Self {
		self.properties = properties;
		self
	}

	pub fn with_children(mut self, children: Vec<Snapshot>) -> Self {
		self.children = children;
		self
	}

	// Overwriting snapshot fields

	pub fn set_id(&mut self, id: Ref) {
		self.id = Some(id);
	}

	pub fn set_file_type(&mut self, file_type: FileType) {
		self.file_type = Some(file_type);
	}

	pub fn set_meta(&mut self, meta: Meta) {
		self.meta = Some(meta);
	}

	pub fn set_path(&mut self, path: &Path) {
		self.path = Some(path.into());
	}

	pub fn set_name(&mut self, name: &str) {
		self.name = name.into();
	}

	pub fn set_class(&mut self, class: &str) {
		self.class = class.into();
	}

	pub fn set_properties(&mut self, properties: HashMap<String, Variant>) {
		self.properties = properties;
	}

	pub fn set_children(&mut self, children: Vec<Snapshot>) {
		self.children = children;
	}

	// Adding to snapshot fields

	pub fn add_property(&mut self, name: &str, value: Variant) {
		self.properties.insert(name.into(), value);
	}

	pub fn add_child(&mut self, child: Snapshot) {
		self.children.push(child);
	}

	// Joining snapshot fields

	pub fn extend_properties(&mut self, properties: HashMap<String, Variant>) {
		self.properties.extend(properties);
	}

	pub fn extend_children(&mut self, children: Vec<Snapshot>) {
		self.children.extend(children);
	}

	pub fn extend_meta(&mut self, meta: Meta) {
		if let Some(snapshot_meta) = &mut self.meta {
			snapshot_meta.extend(meta);
		} else {
			self.meta = Some(meta);
		}
	}

	// Misc

	pub fn apply_project_data(mut self, meta: &Meta, path: &Path) -> Self {
		if let Some(project_data) = &meta.project_data {
			if path != project_data.applies_to {
				return self;
			}

			self.set_name(&project_data.name);

			if let Some(class) = &project_data.class {
				self.set_class(class);
			}

			if let Some(properties) = &project_data.properties {
				self.extend_properties(properties.clone());
			}
		}

		self
	}
}

impl Debug for Snapshot {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let mut debug = f.debug_struct("Snapshot");

		debug.field("name", &self.name);
		debug.field("class", &self.class);

		if let Some(path) = &self.path {
			debug.field("path", &path);
		}

		if let Some(id) = self.id {
			debug.field("id", &id);
		}

		if let Some(meta) = &self.meta {
			debug.field("meta", &meta);
		}

		if !self.properties.is_empty() {
			let mut properties = self.properties.clone();

			if let Some(property) = properties.get_mut("Source") {
				if let Variant::String(source) = property {
					*property = Variant::String(format!("Truncated... ({} lines)", source.lines().count()));
				}
			}

			debug.field("properties", &properties);
		}

		if !self.children.is_empty() {
			debug.field("children", &self.children);
		}

		debug.finish()
	}
}
