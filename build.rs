#[allow(dead_code)]
#[path = "src/identity.rs"]
mod identity;
fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=src/identity.rs");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("app.manifest");
        // Embed in the executable so Windows applies DPI awareness before any
        // window, including early error dialogs, can be created.
        println!("cargo:rustc-link-arg-bin=desksteps=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=desksteps=/MANIFESTINPUT:{}",
            manifest.display()
        );
        resources();
    }
}

fn resources() {
    use std::{fs, path::PathBuf, process::Command};
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let icon = write_icon(&out, false);
    let development_icon = write_icon(&out, true);
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let numeric_version = version.replace('.', ",") + ",0";
    let rc = out.join("DeskSteps.rc");
    let res = out.join("DeskSteps.res");
    let resource = format!(
        r#"1 ICON "{}"
2 ICON "{}"
1 VERSIONINFO
FILEVERSION {numeric_version}
PRODUCTVERSION {numeric_version}
FILEOS 0x40004L
FILETYPE 1L
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904b0"
  BEGIN
   VALUE "CompanyName", "Georgi Petrov\0"
   VALUE "FileDescription", "DeskSteps\0"
   VALUE "FileVersion", "{version}\0"
   VALUE "InternalName", "DeskSteps\0"
   VALUE "LegalCopyright", "Copyright (c) 2026 Georgi Petrov\0"
   VALUE "OriginalFilename", "desksteps.exe\0"
   VALUE "ProductName", "DeskSteps\0"
   VALUE "ProductVersion", "{version}\0"
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x409, 1200
 END
END
"#,
        icon.display().to_string().replace('\\', "/"),
        development_icon.display().to_string().replace('\\', "/")
    );
    fs::write(&rc, resource).unwrap();
    let kit = PathBuf::from(
        std::env::var_os("ProgramFiles(x86)").expect("Windows SDK requires ProgramFiles(x86)"),
    )
    .join("Windows Kits/10/bin");
    let mut candidates = fs::read_dir(kit)
        .expect("Install the Windows SDK")
        .filter_map(Result::ok)
        .map(|e| e.path().join("x64/rc.exe"))
        .filter(|p| p.is_file())
        .collect::<Vec<_>>();
    candidates.sort();
    let compiler = candidates.last().expect("Windows SDK rc.exe is missing");
    let result = Command::new(compiler)
        .arg("/nologo")
        .arg(format!("/fo{}", res.display()))
        .arg(rc)
        .status()
        .expect("Cannot run resource compiler");
    assert!(result.success(), "Resource compiler failed");
    println!("cargo:rustc-link-arg-bin=desksteps={}", res.display());
}

fn write_icon(out: &std::path::Path, development: bool) -> std::path::PathBuf {
    use std::fs;
    let icon = out.join(if development {
        "DeskStepsDevelopment.ico"
    } else {
        "DeskSteps.ico"
    });
    let mut images = vec![];
    for size in [32u32, 48, 256] {
        let mut bitmap = vec![];
        bitmap.extend(40u32.to_le_bytes());
        bitmap.extend(size.to_le_bytes());
        bitmap.extend((size * 2).to_le_bytes());
        bitmap.extend(1u16.to_le_bytes());
        bitmap.extend(32u16.to_le_bytes());
        bitmap.extend([0u8; 24]);
        for y in (0..size).rev() {
            for x in 0..size {
                let px = x * 32 / size;
                let py = y * 32 / size;
                bitmap.extend(identity::pixel(px, py, development));
            }
        }
        bitmap.resize(
            bitmap.len() + ((size as usize).div_ceil(32) * 4) * size as usize,
            0,
        );
        images.push((size, bitmap));
    }
    let mut bytes = vec![0, 0, 1, 0, images.len() as u8, 0];
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, image) in &images {
        bytes.extend([*size as u8, *size as u8, 0, 0]);
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(32u16.to_le_bytes());
        bytes.extend((image.len() as u32).to_le_bytes());
        bytes.extend(offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for (_, image) in images {
        bytes.extend(image);
    }
    fs::write(&icon, bytes).unwrap();
    icon
}
