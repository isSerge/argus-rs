//! A guard that manages the lifecycle of a Docker Compose setup.
//! It runs `docker compose up` on creation and `docker compose down` on drop.
//!
//! Each guard instance is given a unique project name (`-p <name>`) so that
//! concurrent test binaries using the same compose file do not share containers
//! and cannot accidentally tear each other's services down on drop.

use std::process::Command;

pub struct DockerComposeGuard {
    file: String,
    project: String,
}

impl DockerComposeGuard {
    pub fn new(file: &str) -> Self {
        // Derive a project name that is unique per process and compose file so
        // that parallel test binaries never share the same compose project.
        let slug = file.replace(['/', '\\', '.'], "-");
        let project = format!("argus-test-{}-{}", std::process::id(), slug);
        let guard = Self { file: file.to_string(), project };
        guard.up();
        guard
    }

    fn up(&self) {
        let status = Command::new("docker")
            .args(["compose", "-p", &self.project, "-f", &self.file, "up", "-d", "--wait"])
            .status()
            .expect("Failed to execute docker compose up");
        assert!(status.success(), "Docker compose up failed: services did not become healthy");
    }

    fn down(&self) {
        let status = Command::new("docker")
            .args(["compose", "-p", &self.project, "-f", &self.file, "down"])
            .status()
            .expect("Failed to execute docker compose down");
        assert!(status.success(), "Docker compose down failed");
    }
}

impl Drop for DockerComposeGuard {
    fn drop(&mut self) {
        self.down();
    }
}
