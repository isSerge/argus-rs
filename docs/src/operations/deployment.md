# Deployment

This guide covers deploying Argus using Docker, which is the recommended method for production and development environments.

## Docker Deployment

The provided `Dockerfile` and `docker compose.yml` are designed to make deployment straightforward and portable.

### Building the Docker Image

The repository includes a multi-stage `Dockerfile` that uses `cargo-chef` to optimize build times by caching dependencies.

To build the image manually, run the following command from the project root:
```bash
docker build -t argus-rs .
```
The GitHub repository is also configured with a [GitHub Action](../.github/workflows/docker-publish.yml) to automatically build and publish a multi-platform (`linux/amd64`, `linux/arm64`) image to the GitHub Container Registry (GHCR) on every push to the `main` branch.

### Running with Docker Compose (Recommended)

The `docker compose.yml` file is the easiest way to run the application.

#### Setup

1.  **Create a `.env` file**: Copy the `.env.example` to `.env` and fill in your notifier secrets (API tokens, webhook URLs, etc.).
    ```bash
    cp .env.example .env
    ```
2.  **Create a `data` directory**: This directory will be mounted into the container to persist the SQLite database.
    ```bash
    mkdir -p data
    ```
3.  **Configure `monitors.yaml`, `notifiers.yaml`, and `app.yaml`**: Edit the files in the `configs/` or other specified directory to define your monitors and notifiers.

#### Commands

-   **Start the service:**
    ```bash
    docker compose up -d
    ```
-   **View logs:**
    ```bash
    docker compose logs -f
    ```
-   **Stop the service:**
    ```bash
    docker compose down
    ```

### Running with `docker run` (Manual)

If you prefer not to use Docker Compose, you can run the application using a `docker run` command. This is more verbose but offers the same functionality.

```bash
docker run --rm -d \
  --name argus_app \
  --env-file .env \
  -v "$(pwd)/configs:/app/configs:ro" \
  -v "$(pwd)/abis:/app/abis:ro" \
  -v "$(pwd)/data:/app" \
  ghcr.io/isserge/argus-rs:latest run --config-dir /app/configs
```

**Explanation of flags:**
- `--rm`: Automatically remove the container when it exits.
- `-d`: Run in detached mode (in the background).
- `--name argus_app`: Assign a name to the container.
- `--env-file .env`: Load environment variables from the `.env` file.
- `-v "$(pwd)/...:/app/..."`: Mount local directories for configuration and data persistence. The `:ro` flag makes the `configs` and `abis` directories read-only inside the container.
- `ghcr.io/isserge/argus-rs:latest`: The Docker image to use.
- `run --config-dir /app/configs`: The command to execute inside the container.
