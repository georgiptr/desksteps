//! Developer-only checks; never opens a Bluetooth connection.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn run(command: &mut Command) -> Result<Output, String> {
    let output = command.output().map_err(|e| e.to_string())?;
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("{\"reason\":"))
    {
        println!("{line}");
    }
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(format!("Command failed: {command:?}"));
    }
    Ok(output)
}
fn main() {
    if let Err(error) = work() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn work() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    std::env::set_current_dir(root).map_err(|e| e.to_string())?;
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("quality") => quality(
            root,
            args.iter().any(|s| s == "--offline"),
            args.iter().any(|s| s == "--headless"),
        ),
        Some("coverage-gate") => {
            let report = args.get(1).ok_or("Pass annotated LLVM report")?;
            gate(
                root,
                &fs::read_to_string(report).map_err(|e| e.to_string())?,
            )
        }
        Some("package") => {
            let mut cmd = Command::new("powershell.exe");
            cmd.args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "scripts/package.ps1",
            ]);
            cmd.args(&args[1..]);
            run(&mut cmd).map(|_| ())
        }
        _ => Err(
            "Use cargo xtask quality [--offline], coverage-gate REPORT, or package [-Iscc PATH]"
                .into(),
        ),
    }
}
fn quality(root: &Path, offline: bool, headless: bool) -> Result<(), String> {
    run(Command::new("cargo").args(["fmt", "--all", "--check"]))?;
    let locked = if offline {
        vec!["--locked", "--offline"]
    } else {
        vec!["--locked"]
    };
    run(Command::new("cargo")
        .args(["clippy", "--workspace", "--all-targets"])
        .args(&locked)
        .args(["--", "-D", "warnings"]))?;
    let rustc = run(Command::new("rustc").args(["--print", "sysroot"]))?;
    let sysroot = String::from_utf8_lossy(&rustc.stdout).trim().to_string();
    let details = run(Command::new("rustc").arg("-vV"))?;
    let details = String::from_utf8_lossy(&details.stdout);
    let host = details
        .lines()
        .find_map(|s| s.strip_prefix("host: "))
        .ok_or("Missing Rust host")?;
    let llvm = Path::new(&sysroot)
        .join("lib/rustlib")
        .join(host)
        .join("bin");
    let profdata = llvm.join("llvm-profdata.exe");
    let cov = llvm.join("llvm-cov.exe");
    if !profdata.is_file() || !cov.is_file() {
        return Err("Install matching coverage tools: rustup component add llvm-tools".into());
    }
    let report = root.join("target/coverage");
    let profiles = report.join("profiles");
    fs::create_dir_all(&profiles).map_err(|e| e.to_string())?;
    // Only remove raw profiles immediately under this fixed project output path.
    for file in fs::read_dir(&profiles).map_err(|e| e.to_string())? {
        let file = file.map_err(|e| e.to_string())?;
        if file.path().extension().is_some_and(|e| e == "profraw") {
            fs::remove_file(file.path()).map_err(|e| e.to_string())?;
        }
    }
    let target = report.join("build");
    let flags = "-C target-feature=+crt-static -C instrument-coverage";
    let pattern = profiles.join("%p-%m.profraw");
    let mut test_command = Command::new("cargo");
    test_command
        .args(["test", "--workspace", "--message-format=json"])
        .args(&locked)
        .arg("--target-dir")
        .arg(&target)
        .env("RUSTFLAGS", flags)
        .env("LLVM_PROFILE_FILE", &pattern);
    if headless {
        test_command.args(["--", "--skip", "native_ui_"]);
    }
    let tests = run(&mut test_command)?;
    let executables = String::from_utf8_lossy(&tests.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["reason"] == "compiler-artifact" && v["profile"]["test"] == true)
        .filter_map(|v| v["executable"].as_str().map(PathBuf::from))
        .collect::<Vec<_>>();
    run(Command::new("cargo")
        .arg("build")
        .args(&locked)
        .arg("--target-dir")
        .arg(&target)
        .env("RUSTFLAGS", flags))?;
    let exe = target.join("debug/desksteps.exe");
    if !headless {
        run(Command::new(&exe)
            .args(["--no-bluetooth", "--smoke-seconds", "1", "--database"])
            .arg(report.join("smoke.sqlite"))
            .env("LLVM_PROFILE_FILE", &pattern))?;
    }
    replay(&exe, &report, &pattern)?;
    let raw = fs::read_dir(&profiles)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "profraw"))
        .collect::<Vec<_>>();
    if executables.is_empty() || raw.is_empty() {
        return Err("Missing coverage executables/profiles".into());
    }
    let merged = report.join("coverage.profdata");
    run(Command::new(profdata)
        .args(["merge", "-sparse"])
        .args(raw)
        .arg("-o")
        .arg(&merged))?;
    let mut cmd = Command::new(cov);
    cmd.arg("show")
        .arg(&exe)
        .arg(format!("-instr-profile={}", merged.display()))
        .args([
            "-show-instantiations=false",
            "-format=text",
            "-ignore-filename-regex=registry|/rustc/|\\.rustup",
        ]);
    for exe in executables {
        cmd.arg("-object").arg(exe);
    }
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let annotated = String::from_utf8_lossy(&output.stdout);
    fs::write(report.join("lines.txt"), annotated.as_bytes()).map_err(|e| e.to_string())?;
    gate(root, &annotated)?;
    println!("Rust-only quality checks passed. No Bluetooth or movement trial ran.");
    Ok(())
}
fn replay(exe: &Path, report: &Path, pattern: &Path) -> Result<(), String> {
    let source = report.join("synthetic.jsonl");
    let db = report.join("replay.sqlite");
    // Remove only this check's synthetic SQLite files, after child processes exit.
    for suffix in ["", "-wal", "-shm"] {
        let path = PathBuf::from(format!("{}{suffix}", db.display()));
        if path.is_file() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    let uuid = "00002acd-0000-1000-8000-00805f9b34fb";
    let rows = [
        serde_json::json!({"kind":"connected","utc":"2026-01-01T10:00:00Z"}),
        serde_json::json!({"kind":"notification","utc":"2026-01-01T10:00:01Z","monotonic_ns":1_000_000_000i64,"uuid":uuid,"hex":"8424f401d2050068000000003a05060900"}),
        serde_json::json!({"kind":"notification","utc":"2026-01-01T10:00:02Z","monotonic_ns":2_000_000_000i64,"uuid":uuid,"hex":"00"}),
    ];
    fs::write(
        &source,
        rows.iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .map_err(|e| e.to_string())?;
    for expected in [3, 0] {
        run(Command::new(exe)
            .arg("--replay")
            .arg(&source)
            .arg("--database")
            .arg(&db)
            .env("LLVM_PROFILE_FILE", pattern))?;
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(db.with_extension("replay-report.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if value["inserted_events"] != expected || value["decode_errors"] != 1 {
            return Err("Synthetic replay rejection/idempotence failed".into());
        }
    }
    Ok(())
}
fn counts(report: &str) -> BTreeMap<PathBuf, BTreeMap<usize, bool>> {
    let mut files: BTreeMap<PathBuf, BTreeMap<usize, bool>> = BTreeMap::new();
    let mut current = None;
    for line in report.lines() {
        if let Some(path) = line.strip_suffix(".rs:") {
            let path = PathBuf::from(format!("{path}.rs"));
            current = Some(path.clone());
            files.entry(path).or_default();
        } else if let Some(path) = &current {
            let cols = line.split('|').collect::<Vec<_>>();
            if cols.len() >= 3
                && let Ok(number) = cols[0].trim().parse::<usize>()
            {
                let count = cols[1].trim().trim_end_matches(['k', 'M', 'G']);
                if let Ok(count) = count.parse::<f64>() {
                    let hit = files.get_mut(path).unwrap().entry(number).or_default();
                    *hit |= count > 0.0;
                }
            }
        }
    }
    files
}
fn percentage(lines: &BTreeMap<usize, bool>, source: &str) -> Option<f64> {
    let (hit, total) = production_counts(lines, source);
    (total > 0).then(|| 100.0 * hit as f64 / total as f64)
}
fn sources(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let mut result = Vec::new();
    for entry in fs::read_dir(folder).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() && path.file_name().is_none_or(|n| n != "tests") {
            result.extend(sources(&path)?);
        } else if path.extension().is_some_and(|e| e == "rs")
            && path.file_name().is_none_or(|n| n != "tests.rs")
        {
            result.push(path);
        }
    }
    result.sort();
    Ok(result)
}
fn core_area(name: &str) -> Option<&'static str> {
    match name {
        "protocol.rs" => Some("protocol"),
        "control.rs" | "application/controls.rs" | "application/exchange.rs" => Some("controls"),
        "accounting.rs" => Some("storage"),
        n if n.starts_with("storage/") => Some("storage"),
        n if n.starts_with("statistics/") => Some("statistics"),
        _ => None,
    }
}
fn production_counts(lines: &BTreeMap<usize, bool>, source: &str) -> (usize, usize) {
    let cutoff = source
        .lines()
        .position(|s| s.trim() == "#[cfg(test)]")
        .map(|n| n + 1)
        .unwrap_or(usize::MAX);
    lines
        .iter()
        .filter(|(line, _)| **line < cutoff)
        .fold((0, 0), |(hit, total), (_, covered)| {
            (hit + usize::from(*covered), total + 1)
        })
}
fn validate_areas(areas: &BTreeMap<&str, (usize, usize)>) -> Result<(), String> {
    for name in ["protocol", "controls", "storage", "statistics"] {
        let (hit, total) = areas.get(name).copied().unwrap_or_default();
        if total == 0 || hit as f64 / (total as f64) < 0.95 {
            return Err(format!(
                "Production coverage below 95% or missing: {name}: {hit}/{total}"
            ));
        }
    }
    Ok(())
}
fn gate(root: &Path, report: &str) -> Result<(), String> {
    let files = counts(report);
    let mut summary = serde_json::Map::new();
    let source_root = root.join("src");
    let mut areas: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for path in sources(&source_root)? {
        let name = path
            .strip_prefix(&source_root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let lines = files
            .iter()
            .find(|(p, _)| p.canonicalize().ok() == path.canonicalize().ok())
            .map(|(_, lines)| lines);
        let pct = lines.and_then(|lines| percentage(lines, &source));
        summary.insert(
            name.clone(),
            serde_json::json!({"production_lines_percent":pct}),
        );
        if let Some(area) = core_area(&name) {
            // A module containing only declarations/reexports has no executable
            // lines. Every other core source must be present in LLVM's report.
            let declarations_only = !source.contains("fn ");
            if lines.is_none() && !declarations_only {
                return Err(format!("Missing core source coverage: {name}"));
            }
            let (hit, total) = lines
                .map(|lines| production_counts(lines, &source))
                .unwrap_or_default();
            let counts = areas.entry(area).or_default();
            counts.0 += hit;
            counts.1 += total;
        }
    }
    validate_areas(&areas)?;
    summary.insert("core_areas".into(), serde_json::json!(areas.into_iter().map(|(name,(hit,total))|
        (name, serde_json::json!({"covered":hit,"executable":total,"production_lines_percent":100.0*hit as f64/total as f64})))
        .collect::<BTreeMap<_,_>>()));
    fs::create_dir_all(root.join("target/coverage")).map_err(|e| e.to_string())?;
    fs::write(
        root.join("target/coverage/gates.json"),
        serde_json::to_vec_pretty(&summary).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_bodies_cannot_inflate_production_coverage() {
        let lines = BTreeMap::from([(1, false), (2, false), (4, true), (5, true)]);
        assert_eq!(
            percentage(
                &lines,
                "fn production() {}\n// production\n#[cfg(test)]\nmod tests {}"
            ),
            Some(0.0)
        );
        assert_eq!(percentage(&BTreeMap::new(), ""), None);
    }
    #[test]
    fn annotated_counts_preserve_uncovered_lines_and_units() {
        let files = counts("C:/app/src/a.rs:\n 1| 0|miss\n 2| 1.2k|hit\n 3| |not executable\n");
        assert_eq!(
            files[&PathBuf::from("C:/app/src/a.rs")],
            BTreeMap::from([(1, false), (2, true)])
        );
    }
    #[test]
    fn repeated_llvm_objects_merge_hits_instead_of_overwriting_them() {
        let files = counts(
            "C:/src/a.rs:\n1|1|hit\nC:/src/a.rs:\n1|0|same source in another object\n2|0|miss\n",
        );
        assert_eq!(
            files[&PathBuf::from("C:/src/a.rs")],
            BTreeMap::from([(1, true), (2, false)])
        );
    }
    #[test]
    fn recursive_sources_keep_same_named_modules_distinct_and_skip_test_sources() {
        let folder =
            std::env::temp_dir().join(format!("desksteps-coverage-sources-{}", std::process::id()));
        for relative in ["a/mod.rs", "b/mod.rs", "a/tests.rs", "tests/transport.rs"] {
            let path = folder.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let result = sources(&folder).unwrap();
        assert_eq!(
            result,
            vec![folder.join("a/mod.rs"), folder.join("b/mod.rs")]
        );
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn every_core_area_is_independently_required_to_meet_the_threshold() {
        let mut areas = BTreeMap::from([
            ("protocol", (100, 100)),
            ("controls", (95, 100)),
            ("storage", (99, 100)),
            ("statistics", (96, 100)),
        ]);
        validate_areas(&areas).unwrap();
        areas.insert("statistics", (94, 100));
        assert!(validate_areas(&areas).is_err());
        areas.remove("statistics");
        assert!(validate_areas(&areas).is_err());
        areas.insert("statistics", (0, 0));
        assert!(validate_areas(&areas).is_err());
        assert_eq!(core_area("storage/queries.rs"), Some("storage"));
        assert_eq!(core_area("statistics/sessions.rs"), Some("statistics"));
        assert_eq!(core_area("ui/controls/mod.rs"), None);
    }
    #[test]
    fn production_counts_exclude_the_entire_test_section() {
        let lines = BTreeMap::from([(1, true), (2, false), (4, true), (5, true)]);
        assert_eq!(
            production_counts(&lines, "fn a() {}\nfn b() {}\n#[cfg(test)]\nmod tests {}"),
            (1, 2)
        );
    }
}
