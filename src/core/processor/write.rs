use anyhow::{anyhow, Context as AnyhowContext, Result};
use colored::Colorize;
use log::{error, trace, warn};
use rbx_dom_weak::{types::Ref, Instance};
use std::{
	collections::HashMap,
	path::{Path, PathBuf},
};

use crate::{
	argon_error,
	core::{
		meta::{Meta, NodePath, Source, SourceEntry, SourceKind},
		snapshot::{AddedSnapshot, Snapshot, UpdatedSnapshot},
		tree::Tree,
	},
	ext::PathExt,
	middleware::{data, dir, Middleware},
	project::{Project, ProjectNode},
	syncback::{serialize_properties, validate_properties, verify_name},
	vfs::Vfs,
	Properties,
};

macro_rules! path_exists {
	($path:expr) => {
		argon_error!(
			"Instance with path: {} already exists! Skipping..",
			$path.to_string().bold()
		)
	};
}

macro_rules! bad_name {
	($err:expr) => {
		argon_error!("Instance name is corrupted: {}", $err);
	};
}

macro_rules! filter_warn {
	($id:expr) => {
		warn!("Instance {:?} does not pass syncback filter! Skipping..", $id);
	};
	($id:expr, $path:expr) => {
		warn!(
			"Path: {:?} (source of instance: {:?}) does not pass syncback filter! Skipping..",
			$path, $id
		);
	};
}

pub fn apply_addition(snapshot: AddedSnapshot, tree: &mut Tree, vfs: &Vfs) -> Result<()> {
	trace!("Adding {:?} with parent {:?}", snapshot.id, snapshot.parent);

	if !tree.exists(snapshot.parent) {
		warn!(
			"Attempted to add instance: {:?} whose parent doesn't exist: {:?}",
			snapshot.id, snapshot.parent
		);
		return Ok(());
	}

	let parent_id = snapshot.parent;
	let mut snapshot = snapshot.to_snapshot();
	let mut parent_meta = tree.get_meta(parent_id).unwrap().clone();
	let filter = parent_meta.context.syncback_filter();

	if filter.matches_name(&snapshot.name) || filter.matches_class(&snapshot.class) {
		filter_warn!(snapshot.id);
		return Ok(());
	}

	snapshot.properties = validate_properties(snapshot.properties, filter);

	fn write_instance(
		has_children: bool,
		path: &Path,
		snapshot: &Snapshot,
		parent_meta: &Meta,
		vfs: &Vfs,
	) -> Result<Option<Meta>> {
		if let Err(err) = verify_name(&snapshot.name) {
			bad_name!(err);
			return Ok(None);
		}

		let mut meta = snapshot.meta.clone().with_context(&parent_meta.context);
		let filter = parent_meta.context.syncback_filter();
		let properties = snapshot.properties.clone();

		if let Some(middleware) = Middleware::from_class(&snapshot.class) {
			let file_path = parent_meta
				.context
				.sync_rules_of_type(&middleware)
				.iter()
				.find_map(|rule| rule.locate(path, &snapshot.name, has_children))
				.with_context(|| format!("Failed to locate file path for parent: {}", path.display()))?;

			if has_children {
				if vfs.exists(path) {
					path_exists!(path);
					return Ok(None);
				}

				if filter.matches_path(path) {
					filter_warn!(snapshot.id, path);
					return Ok(None);
				}

				dir::write_dir(path, vfs)?;

				meta.source = Source::child_file(path, &file_path);
			} else {
				if vfs.exists(&file_path) {
					path_exists!(path);
					return Ok(None);
				}

				meta.source = Source::file(&file_path);
			}

			if filter.matches_path(&file_path) {
				filter_warn!(snapshot.id, &file_path);
				return Ok(None);
			}

			let properties = middleware.write(properties, &file_path, vfs)?;

			if !properties.is_empty() {
				let data_path = parent_meta
					.context
					.sync_rules_of_type(&Middleware::InstanceData)
					.iter()
					.find_map(|rule| rule.locate(path, &snapshot.name, has_children))
					.with_context(|| format!("Failed to locate data path for parent: {}", path.display()))?;

				if filter.matches_path(&data_path) {
					filter_warn!(snapshot.id, &data_path);
				} else {
					let data_path = data::write_data(true, &snapshot.class, properties, &data_path, &meta, vfs)?;
					meta.source.set_data(data_path);
				}
			}
		} else {
			if vfs.exists(path) {
				path_exists!(path);
				return Ok(None);
			}

			if filter.matches_path(path) {
				filter_warn!(snapshot.id, path);
				return Ok(None);
			}

			dir::write_dir(path, vfs)?;

			meta.source = Source::directory(path);

			let data_path = parent_meta
				.context
				.sync_rules_of_type(&Middleware::InstanceData)
				.iter()
				.find_map(|rule| rule.locate(path, &snapshot.name, true))
				.with_context(|| format!("Failed to locate data path for parent: {}", path.display()))?;

			if filter.matches_path(&data_path) {
				filter_warn!(snapshot.id, &data_path);
			} else {
				let data_path = data::write_data(false, &snapshot.class, properties, &data_path, &meta, vfs)?;
				meta.source.set_data(data_path);
			}
		}

		Ok(Some(meta))
	}

	fn add_non_project_instances(
		parent_id: Ref,
		parent_path: &Path,
		snapshot: Snapshot,
		parent_meta: &Meta,
		tree: &mut Tree,
		vfs: &Vfs,
	) -> Result<Source> {
		let mut parent_path = parent_path.to_owned();

		// Transform parent instance source from file to folder
		let parent_source = if vfs.is_file(&parent_path) {
			let sync_rule = parent_meta
				.context
				.sync_rules()
				.iter()
				.find(|rule| rule.matches(&parent_path))
				.with_context(|| format!("Failed to find sync rule for path: {}", parent_path.display()))?;

			let name = sync_rule.get_name(&parent_path);
			let folder_path = parent_path.with_file_name(&name);

			if vfs.exists(&folder_path) {
				path_exists!(folder_path);
				return Ok(parent_meta.source.clone());
			}

			let file_path = sync_rule
				.locate(&folder_path, &name, true)
				.with_context(|| format!("Failed to locate file path for parent: {}", folder_path.display()))?;

			let data_paths = if let Some(data) = parent_meta.source.get_data() {
				let new_path = parent_meta
					.context
					.sync_rules_of_type(&Middleware::InstanceData)
					.iter()
					.find_map(|rule| rule.locate(&folder_path, &name, true))
					.with_context(|| format!("Failed to locate data path for parent: {}", folder_path.display()))?;

				Some((data.path().to_owned(), new_path))
			} else {
				None
			};

			let mut source = Source::child_file(&folder_path, &file_path);

			dir::write_dir(&folder_path, vfs)?;
			vfs.rename(&parent_path, &file_path)?;

			if let Some(data_paths) = data_paths {
				source.add_data(&data_paths.1);
				vfs.rename(&data_paths.0, &data_paths.1)?;
			}

			parent_path = folder_path;

			source
		} else {
			parent_meta.source.clone()
		};

		let path = parent_path.join(&snapshot.name);

		if snapshot.children.is_empty() {
			if let Some(meta) = write_instance(false, &path, &snapshot, parent_meta, vfs)? {
				let snapshot = snapshot.with_meta(meta);

				tree.insert_instance_with_ref(snapshot, parent_id);
			}
		} else if let Some(meta) = write_instance(true, &path, &snapshot, parent_meta, vfs)? {
			let snapshot = snapshot.with_meta(meta.clone());

			tree.insert_instance_with_ref(snapshot.clone(), parent_id);

			let filter = meta.context.syncback_filter();

			for mut child in snapshot.children {
				child.properties = validate_properties(child.properties.clone(), filter);
				add_non_project_instances(snapshot.id, &path, child, &meta, tree, vfs)?;
			}
		}

		Ok(parent_source)
	}

	fn add_project_instances(
		parent_id: Ref,
		path: &Path,
		node_path: NodePath,
		mut snapshot: Snapshot,
		parent_node: &mut ProjectNode,
		parent_meta: &Meta,
		tree: &mut Tree,
	) {
		let mut node = ProjectNode {
			class_name: Some(snapshot.class.clone()),
			properties: serialize_properties(&snapshot.class, snapshot.properties.clone()),
			..ProjectNode::default()
		};

		if snapshot.meta.keep_unknowns {
			node.keep_unknowns = Some(true);
		}

		let node_path = node_path.join(&snapshot.name);
		let source = Source::project(&snapshot.name, path, node.clone(), node_path.clone());
		let meta = snapshot
			.meta
			.clone()
			.with_context(&parent_meta.context)
			.with_source(source);

		snapshot.meta = meta;
		tree.insert_instance_with_ref(snapshot.clone(), parent_id);

		let filter = snapshot.meta.context.syncback_filter();

		for mut child in snapshot.children {
			child.properties = validate_properties(child.properties, filter);
			add_project_instances(parent_id, path, node_path.clone(), child, &mut node, parent_meta, tree);
		}

		parent_node.tree.insert(snapshot.name, node);
	}

	match parent_meta.source.get() {
		SourceKind::Path(path) => {
			let parent_source = add_non_project_instances(parent_id, path, snapshot, &parent_meta, tree, vfs)?;

			parent_meta.source = parent_source;
			tree.update_meta(parent_id, parent_meta);
		}
		SourceKind::Project(name, path, node, node_path) => {
			if let Some(custom_path) = &node.path {
				let custom_path = path_clean::clean(path.with_file_name(custom_path));

				let parent_source =
					add_non_project_instances(parent_id, &custom_path, snapshot, &parent_meta, tree, vfs)?;

				let parent_source = Source::project(name, path, node.clone(), node_path.clone())
					.with_relevants(parent_source.relevant().to_owned());

				parent_meta.source = parent_source;
				tree.update_meta(parent_id, parent_meta);
			} else {
				let mut project = Project::load(path)?;

				let node = project
					.find_node_by_path(node_path)
					.context(format!("Failed to find project node with path {:?}", node_path))?;

				add_project_instances(parent_id, path, node_path.clone(), snapshot, node, &parent_meta, tree);

				project.save(path)?;
			}
		}
		SourceKind::None => panic!(
			"Attempted to add instance whose parent has no source: {:?}",
			snapshot.id
		),
	}

	Ok(())
}

pub fn apply_update(snapshot: UpdatedSnapshot, tree: &mut Tree, vfs: &Vfs) -> Result<()> {
	trace!("Updating {:?}", snapshot.id);

	if let Some(instance) = tree.get_instance(snapshot.id) {
		let filter = tree.get_meta(snapshot.id).unwrap().context.syncback_filter();

		if filter.matches_name(&instance.name) || filter.matches_class(&instance.class) {
			filter_warn!(snapshot.id);
			return Ok(());
		}

		if snapshot.name.as_ref().is_some_and(|name| filter.matches_name(name)) {
			filter_warn!(snapshot.id);
			return Ok(());
		}

		if snapshot.class.as_ref().is_some_and(|class| filter.matches_class(class)) {
			filter_warn!(snapshot.id);
			return Ok(());
		}
	} else {
		warn!("Attempted to update instance that doesn't exist: {:?}", snapshot.id);
		return Ok(());
	}

	let mut meta = tree.get_meta(snapshot.id).unwrap().clone();
	let instance = tree.get_instance_mut(snapshot.id).unwrap();

	fn locate_instance_data(name: &str, path: &Path, meta: &Meta, vfs: &Vfs) -> Option<PathBuf> {
		let data_path = if let Some(data) = meta.source.get_data() {
			Some(data.path().to_owned())
		} else {
			meta.context
				.sync_rules_of_type(&Middleware::InstanceData)
				.iter()
				.find_map(|rule| rule.locate(path, name, vfs.is_dir(path)))
		};

		if data_path.is_none() {
			warn!("Failed to locate instance data for {}", path.display())
		}

		data_path
	}

	fn update_non_project_properties(
		path: &Path,
		properties: Properties,
		instance: &mut Instance,
		meta: &mut Meta,
		vfs: &Vfs,
	) -> Result<()> {
		let filter = meta.context.syncback_filter();

		if filter.matches_path(path) {
			filter_warn!(instance.referent(), path);
			return Ok(());
		}

		let properties = validate_properties(properties, filter);

		if let Some(middleware) = Middleware::from_class(&instance.class) {
			let file_path = if let Some(file) = meta.source.get_file() {
				Some(file.path().to_owned())
			} else {
				let file_path = meta
					.context
					.sync_rules_of_type(&middleware)
					.iter()
					.find_map(|rule| rule.locate(path, &instance.name, vfs.is_dir(path)));

				if let Some(file_path) = &file_path {
					meta.source.add_file(file_path);
				}

				file_path
			};

			if let Some(file_path) = file_path {
				let properties = middleware.write(properties.clone(), &file_path, vfs)?;

				if let Some(data_path) = locate_instance_data(&instance.name, path, meta, vfs) {
					let data_path = data::write_data(true, &instance.class, properties, &data_path, meta, vfs)?;
					meta.source.set_data(data_path)
				}
			} else {
				error!("Failed to locate file for path {:?}", path.display());
			}
		} else if let Some(data_path) = locate_instance_data(&instance.name, path, meta, vfs) {
			let data_path = data::write_data(false, &instance.class, properties.clone(), &data_path, meta, vfs)?;
			meta.source.set_data(data_path)
		}

		instance.properties = properties;

		Ok(())
	}

	match meta.source.get().clone() {
		SourceKind::Path(path) => {
			if let Some(properties) = snapshot.properties {
				update_non_project_properties(&path, properties, instance, &mut meta, vfs)?;
			}

			// It has to be done after updating properties as it may change the file path
			if let Some(name) = snapshot.name {
				if let Err(err) = verify_name(&name) {
					bad_name!(err);
					return Ok(());
				}

				let new_path = path.with_file_name(path.get_name().replace(&instance.name, &name));
				*meta.source.get_mut() = SourceKind::Path(new_path.clone());

				if vfs.exists(&new_path) {
					path_exists!(new_path);
					return Ok(());
				}

				let filter = meta.context.syncback_filter();

				if let Some(SourceEntry::Folder(path)) = meta.source.get_folder_mut() {
					let new_path = path.with_file_name(&name);

					if filter.matches_path(path) && filter.matches_path(&new_path) {
						filter_warn!(snapshot.id, path);
					} else {
						vfs.rename(path, &new_path)?;
						*path = new_path;
					}
				} else {
					for mut entry in meta.source.relevant_mut() {
						match &mut entry {
							SourceEntry::File(path) | SourceEntry::Data(path) => {
								let name = path.get_name().replace(&instance.name, &name);
								let new_path = path.with_file_name(name);

								if filter.matches_path(path) && filter.matches_path(&new_path) {
									filter_warn!(snapshot.id, path);
									continue;
								}

								vfs.rename(path, &new_path)?;
								*path = new_path;
							}
							_ => continue,
						}
					}
				}

				instance.name = name;
			}

			tree.update_meta(snapshot.id, meta);

			if let Some(_class) = snapshot.class {
				// You can't change the class of an instance inside Roblox Studio
				unreachable!()
			}

			if let Some(_meta) = snapshot.meta {
				// Currently Argon client does not update meta
				unreachable!()
			}
		}
		SourceKind::Project(name, path, node, node_path) => {
			let mut project = Project::load(&path)?;

			if let Some(properties) = snapshot.properties {
				if let Some(custom_path) = node.path {
					let custom_path = path_clean::clean(path.with_file_name(custom_path));

					update_non_project_properties(&custom_path, properties, instance, &mut meta, vfs)?;

					let node = project
						.find_node_by_path(&node_path)
						.context(format!("Failed to find project node with path {:?}", node_path))?;

					node.properties = HashMap::new();
					node.attributes = None;
					node.tags = vec![];
					node.keep_unknowns = None;
				} else {
					let node = project
						.find_node_by_path(&node_path)
						.context(format!("Failed to find project node with path {:?}", node_path))?;

					let class = node.class_name.as_ref().unwrap_or(&name);
					let properties = validate_properties(properties, meta.context.syncback_filter());

					node.properties = serialize_properties(class, properties.clone());
					node.tags = vec![];
					node.keep_unknowns = None;

					instance.properties = properties;
				}
			}

			// It has to be done after updating properties as it may change the node path
			if let Some(new_name) = snapshot.name {
				let parent_node = project.find_node_by_path(&node_path.parent()).with_context(|| {
					format!("Failed to find parent project node with path {:?}", node_path.parent())
				})?;

				let node = parent_node
					.tree
					.remove(&name)
					.context(format!("Failed to remove project node with path {:?}", node_path))?;

				parent_node.tree.insert(new_name.clone(), node.clone());

				let node_path = node_path.parent().join(&new_name);

				*meta.source.get_mut() = SourceKind::Project(new_name.clone(), path.clone(), node, node_path);

				instance.name = new_name;
			}

			tree.update_meta(snapshot.id, meta);
			project.save(&path)?;

			if let Some(_class) = snapshot.class {
				// You can't change the class of an instance inside Roblox Studio
				unreachable!()
			}

			if let Some(_meta) = snapshot.meta {
				// Currently Argon client does not update meta
				unreachable!()
			}
		}
		SourceKind::None => panic!("Attempted to update instance with no source: {:?}", snapshot.id),
	}

	Ok(())
}

pub fn apply_removal(id: Ref, tree: &mut Tree, vfs: &Vfs) -> Result<()> {
	trace!("Removing {:?}", id);

	if let Some(instance) = tree.get_instance(id) {
		let filter = tree.get_meta(id).unwrap().context.syncback_filter();

		if filter.matches_name(&instance.name) || filter.matches_class(&instance.class) {
			filter_warn!(id);
			return Ok(());
		}
	} else {
		warn!("Attempted to remove instance that doesn't exist: {:?}", id);
		return Ok(());
	}

	let meta = tree.get_meta(id).unwrap().clone();

	fn remove_non_project_instances(id: Ref, meta: &Meta, tree: &mut Tree, vfs: &Vfs) -> Result<()> {
		let filter = meta.context.syncback_filter();

		for entry in meta.source.relevant() {
			match entry {
				SourceEntry::Project(_) => continue,
				_ => {
					let path = entry.path();

					if vfs.exists(path) {
						if filter.matches_path(path) {
							filter_warn!(id, path);
						} else {
							vfs.remove(path)?
						}
					}
				}
			}
		}

		// Transform parent instance source from folder to file
		// if it no longer has any children

		let parent = tree
			.get_instance(id)
			.and_then(|instance| tree.get_instance(instance.parent()))
			.expect("Instance has no parent or parent does not have associated meta");

		if parent.children().len() != 1 {
			return Ok(());
		}

		let meta = tree.get_meta_mut(parent.referent()).unwrap();

		if let SourceKind::Path(folder_path) = meta.source.get() {
			let name = folder_path.get_name();

			if let Some(file) = meta.source.get_file() {
				let file_path = meta
					.context
					.sync_rules()
					.iter()
					.find(|rule| rule.matches_child(file.path()))
					.and_then(|rule| rule.locate(folder_path, name, false));

				if let Some(new_path) = file_path {
					vfs.rename(file.path(), &new_path)?;
					let mut source = Source::file(&new_path);

					if let Some(data) = meta.source.get_data() {
						let data_path = meta
							.context
							.sync_rules_of_type(&Middleware::InstanceData)
							.iter()
							.find_map(|rule| rule.locate(folder_path, name, false));

						if let Some(new_path) = data_path {
							vfs.rename(data.path(), &new_path)?;
							source.add_data(&new_path);
						}
					}

					vfs.remove(folder_path)?;
					meta.source = source;
				}
			}
		}

		Ok(())
	}

	match meta.source.get() {
		SourceKind::Path(_) => remove_non_project_instances(id, &meta, tree, vfs)?,
		SourceKind::Project(name, path, node, node_path) => {
			let mut project = Project::load(path)?;
			let parent_node = project.find_node_by_path(&node_path.parent());

			parent_node.and_then(|node| node.tree.remove(name)).ok_or(anyhow!(
				"Failed to remove instance {:?} from project: {:?}",
				id,
				project
			))?;

			if node.path.is_some() {
				remove_non_project_instances(id, &meta, tree, vfs)?;
			}

			project.save(path)?;
		}
		SourceKind::None => panic!("Attempted to remove instance with no source: {:?}", id),
	}

	tree.remove_instance(id);

	Ok(())
}
