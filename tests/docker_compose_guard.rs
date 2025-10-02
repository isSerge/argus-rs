//! A guard that manages the lifecycle of a Docker Compose setup.
//! It runs `docker compose up` on creation and `docker compose down` on drop.

use std::{process::Command, time::Duration};

pub struct DockerComposeGuard {
    file: String,
}

impl DockerComposeGuard {
    pub fn new(file: &str) -> Self {
        let guard = Self { file: file.to_string() };
        guard.up();
        guard
    }

    fn up(&self) {
        let status = Command::new("docker")
            .args(["compose", "-f", &self.file, "up", "-d"])
            .status()
            .expect("Failed to execute docker compose up");
        assert!(status.success(), "Docker compose up failed");
        // Give services some time to start up
        std::thread::sleep(Duration::from_secs(15));
    }

    fn down(&self) {
        let status = Command::new("docker")
            .args(["compose", "-f", &self.file, "down"])
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
