//! Integration tests for the sandbox-safety guard: verifies that paths
//! overlapping the (faked) real caco home are rejected.

use caco_mcp::error::CacoMcpError;
use caco_mcp::sandbox::SandboxPaths;
use tempfile::TempDir;

#[test]
fn rejects_sandbox_equal_to_fake_caco_home() {
    let fake = TempDir::new().unwrap();
    let caco_home = fake.path().join("caco");
    std::fs::create_dir_all(&caco_home).unwrap();
    temp_env::with_var("XDG_DATA_HOME", Some(fake.path().to_str().unwrap()), || {
        let err = SandboxPaths::new(caco_home.clone(), fake.path().to_path_buf()).unwrap_err();
        assert!(matches!(err, CacoMcpError::SandboxPathUnsafe { .. }));
    });
}

#[test]
fn rejects_sandbox_inside_fake_caco_home() {
    let fake = TempDir::new().unwrap();
    let caco_home = fake.path().join("caco");
    let nested = caco_home.join("some-subdir");
    std::fs::create_dir_all(&nested).unwrap();
    temp_env::with_var("XDG_DATA_HOME", Some(fake.path().to_str().unwrap()), || {
        let err = SandboxPaths::new(nested.clone(), fake.path().to_path_buf()).unwrap_err();
        assert!(matches!(err, CacoMcpError::SandboxPathUnsafe { .. }));
    });
}

#[test]
fn rejects_sandbox_equal_to_env_caco_home() {
    let fake = TempDir::new().unwrap();
    temp_env::with_vars(
        [
            (
                "XDG_DATA_HOME",
                Some("/nonexistent/xdg-sandbox-safety-tests"),
            ),
            ("CACO_HOME", Some(fake.path().to_str().unwrap())),
        ],
        || {
            let err = SandboxPaths::new(fake.path().to_path_buf(), fake.path().to_path_buf())
                .unwrap_err();
            assert!(matches!(err, CacoMcpError::SandboxPathUnsafe { .. }));
        },
    );
}
