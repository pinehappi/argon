use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use log::{info, trace};
use roblox_install::RobloxStudio;
use std::{
	env, fs,
	path::PathBuf,
	process::{self, Command},
	sync::mpsc,
};

use crate::{
	argon_info,
	config::Config,
	core::Core,
	exit,
	program::{Program, ProgramKind},
	project::{self, Project},
	sessions, util,
};

/// Build project into Roblox place or model
#[derive(Parser)]
pub struct Build {
	/// Project path
	#[arg()]
	project: Option<PathBuf>,

	/// Session indentifier
	#[arg()]
	session: Option<String>,

	/// Output path
	#[arg(short, long)]
	output: Option<PathBuf>,

	/// Build plugin and place it into plugins folder
	#[arg(short, long)]
	plugin: bool,

	/// Whether to build in XML format (.rbxlx or .rbxmx)
	#[arg(short, long)]
	xml: bool,

	/// Whether to build using roblox-ts
	#[arg(short, long)]
	ts: bool,

	/// Rebuild project every time files change
	#[arg(short, long)]
	watch: bool,

	/// Spawn the Argon child process
	#[arg(long, hide = true)]
	argon_spawn: bool,
}

impl Build {
	pub fn main(self) -> Result<()> {
		let config = Config::load();

		if self.watch && !self.argon_spawn && config.spawn() {
			return self.spawn();
		}

		let project = self.project.clone().unwrap_or_default();
		let project_path = project::resolve(project, config.project_name())?;

		if !project_path.exists() {
			exit!("Project {} does not exist", project_path.to_str().unwrap().bold());
		}

		let project = Project::load(&project_path)?;
		let is_ts = project.is_ts();

		let mut xml = self.xml;
		let path = if self.plugin {
			if project.is_place() {
				exit!("Cannot build plugin from place project");
			}

			let plugins_path = RobloxStudio::locate()?.plugins_path().to_owned();
			let ext = if xml { "rbxmx" } else { "rbxm" };

			plugins_path.join(format!("{}.{}", project.name, ext))
		} else if let Some(path) = self.output.clone() {
			if path.is_dir() {
				path.join(self.get_default_file(&project))
			} else {
				let ext = util::get_file_ext(&path);

				if ext.is_empty() {
					fs::create_dir_all(&path)?;

					path.join(self.get_default_file(&project))
				} else {
					if ext == "rbxlx" || ext == "rbxmx" {
						xml = true;
					} else if ext == "rbxl" || ext == "rbxm" {
						xml = false;
					} else {
						exit!(
							"Invalid file extension: {}. Only {}, {}, {}, {} extensions are allowed.",
							ext,
							"rbxl".bold(),
							"rbxlx".bold(),
							"rbxm".bold(),
							"rbxmx".bold(),
						);
					}

					if ext.starts_with("rbxm") && project.is_place() {
						exit!("Cannot build model or plugin from place project");
					} else if ext.starts_with("rbxl") && !project.is_place() {
						exit!("Cannot build place from plugin or model project");
					}

					let parent = path.parent().unwrap();

					if !parent.exists() {
						fs::create_dir_all(parent)?;
					}

					path
				}
			}
		} else {
			self.get_default_file(&project)
		};

		if is_ts {
			argon_info!("Compiling TypeScript files...");

			let working_dir = project_path.parent().unwrap();

			let child = Program::new(ProgramKind::Npx)
				.message("Failed to start roblox-ts")
				.current_dir(working_dir)
				.arg("rbxtsc")
				.arg("build")
				.spawn()?;

			if let Some(mut child) = child {
				child.wait()?;
			} else {
				return Ok(());
			}
		}

		let mut core = Core::new(config.clone(), project)?;

		core.load_dom()?;
		core.build(&path, xml)?;

		argon_info!(
			"Successfully built project: {} to: {}",
			project_path.to_str().unwrap().bold(),
			path.to_str().unwrap().bold()
		);

		if self.watch {
			if is_ts {
				trace!("Starting roblox-ts");

				let working_dir = project_path.parent().unwrap();

				let mut child = Program::new(ProgramKind::Npx)
					.current_dir(working_dir)
					.arg("rbxtsc")
					.arg("--watch")
					.spawn()?
					.unwrap();

				util::handle_kill(move || {
					child.kill().ok();
				})?;
			}

			if config.spawn() {
				sessions::add(self.session, None, None, process::id())?;
			}

			let (sender, receiver) = mpsc::channel();

			core.watch(Some(sender));

			argon_info!("Watching for changes...");

			for _ in receiver {
				info!("Rebuilding project...");

				core.build(&path, xml)?;
			}
		}

		Ok(())
	}

	fn get_default_file(&self, project: &Project) -> PathBuf {
		let ext = if project.is_place() {
			if self.xml {
				"rbxlx"
			} else {
				"rbxl"
			}
		} else if self.xml {
			"rbxmx"
		} else {
			"rbxm"
		};

		PathBuf::from(format!("{}.{}", project.name, ext))
	}

	fn spawn(self) -> Result<()> {
		let program = env::current_exe().unwrap_or(PathBuf::from("argon"));

		let log_style = env::var("RUST_LOG_STYLE").unwrap_or("auto".to_string());
		let backtrace = env::var("RUST_BACKTRACE").unwrap_or("0".to_string());

		let mut args = vec![String::from("build"), util::get_verbosity_flag()];

		if let Some(project) = self.project {
			args.push(util::path_to_string(&project))
		}

		if let Some(output) = self.output {
			args.push(util::path_to_string(&output))
		}

		if self.plugin {
			args.push(String::from("--plugin"))
		}

		if self.xml {
			args.push(String::from("--xml"))
		}

		if self.ts {
			args.push(String::from("--ts"))
		}

		if self.watch {
			args.push(String::from("--watch"))
		}

		Command::new(program)
			.args(args)
			.arg("--yes")
			.arg("--argon-spawn")
			.env("RUST_LOG_STYLE", log_style)
			.env("RUST_BACKTRACE", backtrace)
			.spawn()?;

		Ok(())
	}
}