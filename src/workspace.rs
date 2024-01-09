use anyhow::{bail, Result};
use chrono::Datelike;
use log::trace;
use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use crate::{
	argon_info,
	program::{self, Program},
	util,
};

pub fn init(project: &Path, template: &str, source: &String, git: bool, docs: bool) -> Result<()> {
	let home_dir = util::get_home_dir()?;
	let template_dir = home_dir.join(".argon").join("templates").join(template);

	if !template_dir.exists() {
		bail!("Template {} does not exist", template)
	}

	let project_name = get_name(project);
	let workspace_dir = get_dir(project);

	if !workspace_dir.exists() {
		fs::create_dir_all(&workspace_dir)?;
	}

	for entry in fs::read_dir(template_dir)? {
		let entry = entry?;

		let path = entry.path();
		let name = util::get_file_name(&path);
		let stem = util::get_file_stem(&path);

		let new_path = if name == "project.json" {
			project.to_owned()
		} else {
			workspace_dir.join(name)
		};

		if new_path.exists() {
			continue;
		}

		match name {
			"project.json" => {
				let content = fs::read_to_string(path)?;
				let content = content.replace("$name", &project_name);
				let content = content.replace("$src", source);

				fs::write(new_path, content)?;
			}
			".gitignore" => {
				if git {
					fs::copy(path, new_path)?;
				}
			}
			"wally.toml" => {
				let content = fs::read_to_string(path)?;
				let content = content.replace("$name", &project_name);
				let content = content.replace("$author", &util::get_username());

				fs::write(new_path, content)?;
			}
			_ => match stem {
				"README" | "CHANGELOG" => {
					if docs {
						let content = fs::read_to_string(path)?;
						let content = content.replace("$name", &project_name);

						fs::write(new_path, content)?;
					}
				}
				"LICENSE" => {
					if docs {
						let name = util::get_username();
						let year = chrono::Utc::now().year();

						let content = fs::read_to_string(path)?;
						let content = content.replace("[yyyy]", &year.to_string());
						let content = content.replace("[owner]", &name);

						fs::write(new_path, content)?;
					}
				}
				_ => {
					fs::copy(path, new_path)?;
				}
			},
		}
	}

	let source_dir = workspace_dir.join(source);

	if !source_dir.exists() {
		fs::create_dir(source_dir)?;
	}

	Ok(())
}

pub fn init_ts(path: &Path, template: &str, git: bool) -> Result<bool> {
	argon_info!("Waiting for npm...");

	let command = match template {
		"game" => template,
		"plugin" => template,
		"package" => template,
		"model" => template,
		_ => "init",
	};

	let child = program::spawn(
		Command::new(program::NPM)
			.arg("init")
			.arg("roblox-ts")
			.arg(command)
			.arg("--")
			.arg("--yes")
			.arg("--skipBuild")
			.arg(format!("--git={}", git))
			.args(["--dir", &util::path_to_string(path)])
			.spawn(),
		Program::Npm,
		"Failed to initialize roblox-ts project",
	)?;

	if let Some(child) = child {
		let output = child.wait_with_output()?;

		if let Some(code) = output.status.code() {
			return Ok(code == 0);
		}

		Ok(false)
	} else {
		trace!("npm is not installed");
		Ok(false)
	}
}

pub fn initialize_repo(directory: &PathBuf) -> Result<()> {
	let output = program::output(
		Command::new("git").arg("init").arg(directory).output(),
		Program::Git,
		"Failed to initialize repository",
	)?;

	if output.is_some() {
		trace!("Initialized Git repository");
	} else {
		trace!("Git is not installed");
	}

	Ok(())
}

pub fn get_dir(project_path: &Path) -> PathBuf {
	let mut workspace_dir = project_path.to_owned();
	workspace_dir.pop();

	workspace_dir
}

pub fn get_name(project_path: &Path) -> String {
	let mut name = project_path.to_owned();
	name.pop();

	name.file_name().unwrap().to_str().unwrap().to_owned()
}
