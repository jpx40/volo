use std::{fs::create_dir_all, path::PathBuf, process::Command};

use clap::{value_parser, Parser};
use volo_build::{
    config_builder::InitBuilder,
    model::{Entry, GitSource, Idl, Source, DEFAULT_FILENAME},
    util::{get_repo_latest_commit_id, git_repo_init, strip_slash_prefix, DEFAULT_CONFIG_FILE},
};

use crate::command::CliCommand;

#[derive(Parser, Debug)]
#[command(about = "init your thrift or grpc project")]
pub struct Init {
    #[arg(help = "The name of project")]
    pub name: String,
    #[arg(
        short = 'g',
        long = "git",
        help = "Specify the git repo for idl.\nShould be in the format of \
                \"git@domain:path/repo.git\".\nExample: git@github.com:cloudwego/volo.git"
    )]
    pub git: Option<String>,
    #[arg(
        short = 'r',
        long = "ref",
        requires = "git",
        help = "Specify the git repo ref(branch) for idl.\nExample: main / $TAG"
    )]
    pub r#ref: Option<String>,

    #[arg(
        short = 'i',
        long = "includes",
        help = "Specify the include dirs for idl.\nIf -g or --git is specified, then this should \
                be the path in the specified git repo."
    )]
    pub includes: Option<Vec<PathBuf>>,
    #[arg(
        value_parser = value_parser!(PathBuf),
        help = "Specify the path for idl.\nIf -g or --git is specified, then this should be the \
                path in the specified git repo.\nExample: \t-g not \
                specified:\t./idl/server.thrift\n\t\t-g specified:\t\t/path/to/idl/server.thrift"
    )]
    pub idl: PathBuf,
}

impl Init {
    pub fn is_grpc_project(&self) -> bool {
        if let Some(ext) = self.idl.extension() {
            ext == "proto"
        } else {
            false
        }
    }

    fn init_gen(&self, config_entry: Entry) -> anyhow::Result<(String, String)> {
        InitBuilder::new(config_entry).init()
    }

    fn copy_grpc_template(&self, config_entry: Entry) -> anyhow::Result<()> {
        std::env::set_var("OUT_DIR", "/tmp/idl");
        let (service_global_name, methods) = self.init_gen(config_entry)?;

        let name = self.name.replace(['.', '-'], "_");
        let cwd = std::env::current_dir()?;
        let folder = cwd.as_path();

        // root dirs
        crate::templates_to_target_file!(folder, "templates/grpc/gitignore", ".gitignore");
        crate::templates_to_target_file!(
            folder,
            "templates/grpc/cargo_toml",
            "Cargo.toml",
            name = &name
        );

        // src dirs
        create_dir_all(folder.join("src/bin"))?;
        crate::templates_to_target_file!(
            folder,
            "templates/grpc/src/bin/server_rs",
            "src/bin/server.rs",
            name = &name,
            service_global_name = &service_global_name,
        );
        crate::templates_to_target_file!(
            folder,
            "templates/grpc/src/lib_rs",
            "src/lib.rs",
            service_global_name = &service_global_name,
            methods = &methods,
        );

        // volo-gen dirs
        create_dir_all(folder.join("volo-gen/src"))?;
        crate::templates_to_target_file!(
            folder,
            "templates/grpc/volo-gen/build_rs",
            "volo-gen/build.rs"
        );
        crate::templates_to_target_file!(
            folder,
            "templates/grpc/volo-gen/cargo_toml",
            "volo-gen/Cargo.toml",
        );
        crate::templates_to_target_file!(
            folder,
            "templates/grpc/volo-gen/src/lib_rs",
            "volo-gen/src/lib.rs",
        );

        Ok(())
    }

    fn copy_thrift_template(&self, config_entry: Entry) -> anyhow::Result<()> {
        std::env::set_var("OUT_DIR", "/tmp/idl");
        let (service_global_name, methods) = self.init_gen(config_entry)?;

        let name = self.name.replace(['.', '-'], "_");
        let cwd = std::env::current_dir()?;
        let folder = cwd.as_path();

        // root dirs
        crate::templates_to_target_file!(folder, "templates/thrift/gitignore", ".gitignore");
        crate::templates_to_target_file!(
            folder,
            "templates/thrift/cargo_toml",
            "Cargo.toml",
            name = &name
        );

        // src dirs
        create_dir_all(folder.join("src/bin"))?;
        crate::templates_to_target_file!(
            folder,
            "templates/thrift/src/bin/server_rs",
            "src/bin/server.rs",
            name = &name,
            service_global_name = &service_global_name,
        );
        crate::templates_to_target_file!(
            folder,
            "templates/thrift/src/lib_rs",
            "src/lib.rs",
            service_global_name = &service_global_name,
            methods = &methods,
        );

        // volo-gen dirs
        create_dir_all(folder.join("volo-gen/src"))?;
        crate::templates_to_target_file!(
            folder,
            "templates/thrift/volo-gen/build_rs",
            "volo-gen/build.rs"
        );
        crate::templates_to_target_file!(
            folder,
            "templates/thrift/volo-gen/cargo_toml",
            "volo-gen/Cargo.toml",
        );
        crate::templates_to_target_file!(
            folder,
            "templates/thrift/volo-gen/src/lib_rs",
            "volo-gen/src/lib.rs",
        );

        Ok(())
    }
}

impl CliCommand for Init {
    fn run(&self, cx: crate::context::Context) -> anyhow::Result<()> {
        volo_build::util::with_config(|config| {
            let mut lock = None;
            let mut idl = Idl::new();
            idl.includes = self.includes.clone();

            // Handling Git-Based Template Creation
            if let Some(git) = self.git.as_ref() {
                let r#ref = self.r#ref.as_deref().unwrap_or("HEAD");
                let lock_value = get_repo_latest_commit_id(git, r#ref)?;
                let _ = lock.insert(lock_value);
                idl.source = Source::Git(GitSource {
                    repo: git.clone(),
                    r#ref: None,
                    lock,
                });
            }

            if self.git.is_some() {
                idl.path = strip_slash_prefix(&self.idl);
            } else {
                idl.path = self.idl.clone();
                // only ensure readable when idl is from local
                idl.ensure_readable()?;
            }

            let mut entry = Entry {
                protocol: idl.protocol(),
                filename: PathBuf::from(DEFAULT_FILENAME),
                idls: vec![idl.clone()],
                touch_all: false,
                nonstandard_snake_case: false,
            };

            if self.is_grpc_project() {
                self.copy_grpc_template(entry.clone())?;
            } else {
                self.copy_thrift_template(entry.clone())?;
            }

            if self.git.as_ref().is_none() {
                // we will move volo.yml to volo-gen, so we need to add .. to includes and idl path
                if let Some(idl) = entry.idls.get_mut(0) {
                    if let Some(includes) = &mut idl.includes {
                        for i in includes {
                            if i.is_absolute() {
                                continue;
                            }
                            *i = PathBuf::new().join("../").join(i.clone());
                        }
                    }
                    if !idl.path.is_absolute() {
                        idl.path = PathBuf::new().join("../").join(self.idl.clone());
                    }
                }
            }

            let config_entry = config.entries.entry(cx.entry_name);
            match config_entry {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    // find the specified idl and update it.
                    let mut found = false;
                    for idl in e.get_mut().idls.iter_mut() {
                        if self.idl != idl.path {
                            continue;
                        }
                        let (repo, r#ref) = match idl.source {
                            Source::Git(GitSource {
                                ref mut repo,
                                ref mut r#ref,
                                ..
                            }) => {
                                // found the desired idl, update it
                                found = true;
                                (repo, r#ref)
                            }
                            _ => continue,
                        };

                        if let Some(git) = self.git.as_ref() {
                            *repo = git.clone();
                            if self.r#ref.is_some() {
                                *r#ref = self.r#ref.clone();
                            }
                        } else {
                            unreachable!()
                        }
                        break;
                    }

                    if !found {
                        e.get_mut().idls.push(idl);
                    }
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(entry);
                }
            };

            Ok(())
        })?;

        std::fs::rename(
            DEFAULT_CONFIG_FILE,
            PathBuf::from("./volo-gen/").join(DEFAULT_CONFIG_FILE),
        )?;

        let _ = Command::new("cargo").arg("fmt").arg("--all").output()?;

        let cwd = std::env::current_dir()?;
        git_repo_init(&cwd)?;

        Ok(())
    }
}
