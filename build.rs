use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=packaging/linux/awebpinator.desktop.in");
    println!("cargo:rerun-if-changed=packaging/linux/install-awebpinator.sh.in");
    println!("cargo:rerun-if-changed=packaging/icon.png");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing manifest dir"));
    let profile = env::var("PROFILE").expect("missing cargo profile");
    let target_dir = cargo_target_dir(&manifest_dir);
    let build_output_dir = target_dir.join(&profile);

    fs::create_dir_all(&build_output_dir)?;

    let desktop_template =
        fs::read_to_string(manifest_dir.join("packaging/linux/awebpinator.desktop.in"))?;
    let install_template =
        fs::read_to_string(manifest_dir.join("packaging/linux/install-awebpinator.sh.in"))?;
    let icon_source = manifest_dir.join("packaging/icon.png");

    let desktop_output = replace_tokens(&desktop_template, &profile);
    let install_output = replace_tokens(&install_template, &profile);

    fs::write(build_output_dir.join("awebpinator.desktop"), desktop_output)?;
    fs::copy(icon_source, build_output_dir.join("icon.png"))?;
    let install_script_path = build_output_dir.join("install-awebpinator.sh");
    fs::write(&install_script_path, install_output)?;

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&install_script_path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&install_script_path, permissions)?;
    }

    Ok(())
}

fn cargo_target_dir(manifest_dir: &Path) -> PathBuf {
    match env::var_os("CARGO_TARGET_DIR") {
        Some(path) => {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                manifest_dir.join(path)
            }
        }
        None => manifest_dir.join("target"),
    }
}

fn replace_tokens(template: &str, profile: &str) -> String {
    let build_command = if profile == "release" {
        "cargo build --release"
    } else {
        "cargo build"
    };

    template
        .replace("@APP_NAME@", "AWEBPinator")
        .replace("@BIN_NAME@", env!("CARGO_PKG_NAME"))
        .replace("@VERSION@", env!("CARGO_PKG_VERSION"))
        .replace("@PROFILE@", profile)
        .replace("@BUILD_COMMAND@", build_command)
}
