use super::*;
use crate::user_skills::{remove_from, save, sync_one, SkillInput, MANAGED_MARKER, SKILL_FILE};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Cartella temporanea unica per test, rimossa a fine test.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mc-skill-targets-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// Cartella dati con le cartelle plugin di Claude Code per gli ambienti dati.
    fn with_plugins(envs: &[&str]) -> Self {
        let data = Self::new();
        for env in envs {
            fs::create_dir_all(plugins_dir(&data.0, CLAUDE_CODE_ID).join(env)).unwrap();
        }
        data
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn distros_in_use_needs_an_installed_distro_with_a_plugin_folder() {
    let installed = strings(&["Debian", "Ubuntu", "Arch"]);
    let folders = strings(&["Arch", "Ubuntu", "Gone"]);
    assert_eq!(distros_in_use(&folders, &installed), strings(&["Ubuntu", "Arch"]));
    assert!(distros_in_use(&[], &installed).is_empty());
    assert!(distros_in_use(&folders, &[]).is_empty());
}

#[test]
fn distros_in_use_skips_docker_and_invalid_names() {
    let installed = strings(&["docker-desktop", "Docker-Desktop-Data", "bad/name", "Ubuntu"]);
    let folders = strings(&["docker-desktop", "Docker-Desktop-Data", "bad-name", "Ubuntu"]);
    assert_eq!(distros_in_use(&folders, &installed), strings(&["Ubuntu"]));
}

#[test]
fn distros_in_use_matches_the_sanitized_folder_name() {
    assert_eq!(distros_in_use(&strings(&["Ubuntu-22"]), &strings(&["Ubuntu 22"])), strings(&["Ubuntu 22"]));
}

#[test]
fn wsl_skills_dir_is_a_unc_path_under_the_home() {
    let expected = PathBuf::from("\\\\wsl.localhost\\Ubuntu\\home\\u\\.claude\\skills");
    assert_eq!(wsl_skills_dir("Ubuntu", "/home/u"), Ok(expected.clone()));
    assert_eq!(wsl_skills_dir("Ubuntu", "/home/u/"), Ok(expected));
    assert!(wsl_skills_dir("Ubuntu", "home/u").is_err());
}

#[test]
fn plugin_env_dirs_ignores_windows_and_plain_files() {
    let data = TempDir::with_plugins(&["windows", "Ubuntu", "Debian"]);
    fs::write(plugins_dir(&data.0, CLAUDE_CODE_ID).join("stray.txt"), "x").unwrap();
    let mut dirs = plugin_env_dirs(&data.0);
    dirs.sort();
    assert_eq!(dirs, strings(&["Debian", "Ubuntu"]));
    assert!(plugin_env_dirs(&data.0.join("missing")).is_empty());
}

#[test]
fn targets_are_windows_only_without_distro_plugins_and_never_list_distros() {
    let data = TempDir::with_plugins(&["windows"]);
    let targets = targets_with(&data.0, || panic!("wsl -l must not run"), |_| panic!("no home needed"));
    assert_eq!(targets.iter().map(|t| t.env.as_str()).collect::<Vec<_>>(), [WINDOWS_ENV]);
}

#[test]
fn an_unreachable_distro_is_an_error_target_and_does_not_hide_the_others() {
    let data = TempDir::with_plugins(&["windows", "Ubuntu", "Debian", "Unused"]);
    let installed = || Ok(strings(&["Ubuntu", "Debian", "Unused", "docker-desktop"]));
    let home = |d: &str| match d {
        "Debian" => Err("Cannot resolve the home folder of WSL distro 'Debian': timed out".to_string()),
        _ => Ok("/home/me".to_string()),
    };
    let targets = targets_with(&data.0, installed, home);
    let envs: Vec<&str> = targets.iter().map(|t| t.env.as_str()).collect();
    assert_eq!(envs, [WINDOWS_ENV, "Ubuntu", "Debian", "Unused"]);
    assert_eq!(targets[1].dir, Ok(PathBuf::from("\\\\wsl.localhost\\Ubuntu\\home\\me\\.claude\\skills")));
    assert_eq!(
        targets[2].dir.as_ref().unwrap_err(),
        "Cannot resolve the home folder of WSL distro 'Debian': timed out"
    );
    assert!(targets[3].dir.is_ok());
}

#[test]
fn a_failing_distro_listing_leaves_only_windows() {
    let data = TempDir::with_plugins(&["Ubuntu"]);
    let targets = targets_with(&data.0, || Err("wsl missing".to_string()), |_| panic!("no home needed"));
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].env, WINDOWS_ENV);
}

/// End-to-end su una distro reale, in una cartella usa e getta (non nel vero
/// `~/.claude/skills`): sync via UNC, verifica da entrambi i lati e rimozione.
/// `cargo test -- --ignored wsl_skills_real --nocapture`
#[test]
#[ignore]
fn wsl_skills_real_sync_and_remove_through_unc() {
    let distro = std::env::var("CC_TEST_DISTRO").unwrap_or_else(|_| "Ubuntu-24.04".to_string());
    let home = crate::wsl::linux_home(&distro).expect("home della distro");
    println!("home: {home}");
    let linux_dir = format!("{home}/.cache/cli-companion-skills-test-{}", std::process::id());
    let unc_dir = PathBuf::from(wsl_to_windows_path(&distro, &linux_dir).expect("path UNC"));
    let store = TempDir::new();
    let input = SkillInput {
        name: "wsl-probe".to_string(),
        description: "Probe".to_string(),
        body: "Body".to_string(),
        files: Vec::new(),
    };

    let result = std::panic::catch_unwind(|| {
        save(&store.0, &input).unwrap();
        sync_one(&store.0, &input.name, &unc_dir).unwrap();
        let skill = unc_dir.join(&input.name);
        assert!(skill.join(SKILL_FILE).is_file());
        assert!(skill.join(MANAGED_MARKER).is_file());
        assert!(distro_has_file(&distro, &format!("{linux_dir}/{}/{SKILL_FILE}", input.name)));

        assert_eq!(remove_from(&unc_dir, &input.name), Ok(true));
        assert!(!skill.exists());
    });
    let _ = fs::remove_dir_all(&unc_dir);
    result.unwrap();
}

/// `test -f` eseguito dentro la distro: conferma il file dal lato Linux.
fn distro_has_file(distro: &str, linux_file: &str) -> bool {
    crate::wsl::wsl_command()
        .args(["-d", distro, "-e", "test", "-f", linux_file])
        .status()
        .is_ok_and(|s| s.success())
}
