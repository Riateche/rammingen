# Rammingen

**Rammingen** is a self-hosted file synchronization and backup system.

The rammingen client runs periodically in background, scanning local files and uploading any file changes to your server. It also pulls updates from the server and applies them locally, keeping your files in sync.

Supported platforms: **Windows, Linux, macOS, Android**.

## Features

1. **Cross-platform syncing:** A single Rammingen server can work with multiple clients running simultaneously on any supported system. All files are placed in a unified virtual file tree, where files and directories can be referenced by *archive path*.
1. **Fully configurable path management:** Configure multiple local directories for periodic sync and map each of them to an archive path. Each client can have its own independent mapping configuration.
1. **Powerful ignore rules:** Exclude files by name or path using exact matches or regex, with rules configurable globally or per directory.
1. **Extensive command line interface:**
    1. Upload and download files and directories on demand.
    2. Show the history of changes for any file or directory.
    3. Restore a file or directory to an earlier version.
    4. Move or delete archived files remotely.
1. **End-to-end encryption:** File contents and most metadata (including path names) are encrypted client-side using a private key that never leaves your devices. Rammingen uses AES-CMAC-SIV in AEAD mode with a 512-bit key.
1. **Content deduplication:** Identical file contents are stored only once, reducing storage and transfer overhead and improving sync performance when files are moved or renamed.
1. **Compression:** All files are compressed using DEFLATE.
1. **Full version history:** Each version of every file is stored independently. Browse all versions of a file or all historical changes in a directory. Download any specific version of a file or even the entire directory state at a chosen point in time.
1. **Automatic cleanup:** Old versions are periodically pruned while retaining snapshots at configured intervals. The default policy keeps all versions for two weeks and weekly snapshots for older data.
1. **Unattended operation:** Rammingen runs in the background and requires no user intervention. It can show desktop notifications for errors or periodic sync statistics, with configurable intervals.
1. **No unexpected file changes:** Rammingen never modifies file names or contents. It does not create duplicate files during conflicts; instead, the most recent version is applied while all historical versions remain available for recovery.
1. **Optimized for slow networks and modest servers:** The server can run smoothly even on the cheapest VDS/VPS plans.
1. **Performant on millions of files:** Designed with git repositories and large codebases in mind, Rammingen handles large amounts of files efficiently.
1. **Rigorously tested:** A fuzz test simulates real-world user behavior (editing files, syncing, uploading, downloading, and more) across multiple devices to ensure synchronization is correct every time.

## Setup

```mermaid
graph BT
    subgraph Server host
        server[Rammingen server]
        db[("Metadata storage<br>(Postgres database)")]
        storage[("File content storage<br>(local file system)")]
        tls_proxy["TLS proxy (Nginx)"]
    end

    subgraph Client host #1
        client1["Rammingen client"]
        directory1[(Local directory)]
    end

    subgraph Client host #2
        client2["Rammingen client"]
        directory2[(Local directory 1)]
        directory22[(Local directory 2)]
    end

    server --> db
    server --> storage
    client1 --> tls_proxy
    client1 --> directory1
    client2 --> tls_proxy
    client2 --> directory2
    client2 --> directory22
    tls_proxy --> server
```

In order to use Rammingen, you will need a server with a Postgres database and some space in the filesystem for storage. You also need to set up a Rammingen client on your local system.

### Server setup

This guide assumes using Ubuntu on the server.
However, rammingen-server also supports other Linux distributions, Windows, and macOS.
For more information, see also [server README](server/README.md).

1. Install Nginx, Postgres and Docker from system repository:
    ```sh
    sudo apt update
    sudo apt install nginx postgresql docker.io
    ```
1. Set up a Postgres user and database (replace `dbpassword` with a new password):
    ```sh
    sudo -u postgres psql -c "CREATE USER rammingen WITH ENCRYPTED PASSWORD 'dbpassword';"
    sudo -u postgres psql -c "CREATE DATABASE rammingen;"
    sudo -u postgres psql -c "GRANT ALL PRIVILEGES ON DATABASE rammingen TO rammingen;"
    ```
1. Create a local directory for storage:
    ```sh
    sudo mkdir /var/storage
    ```
1. Create a server configuration file at `/etc/rammingen-server.conf`:
    ```json5
    {
        // URL of the database.
        database_url: "postgres://rammingen:dbpassword@127.0.0.1:5432/rammingen",
        // Path to the local file storage.
        storage_path: "/var/storage",
        // IP and port that the server will listen.
        bind_addr: "127.0.0.1:8080",
        // Time between snapshots. A snapshot is a copy of the state
        // of all archive entries at a certain time.
        // Snapshots are not deleted automatically.
        // Supported duration formats: https://docs.rs/humantime/latest/humantime/fn.parse_duration.html
        snapshot_interval: "1week",
        // Time during which all recorded entry versions are stored in the database.
        // Entry versions that are older than `retain_detailed_history_for` will
        // eventually be deleted, except for entry versions that are part of a snapshot.
        retain_detailed_history_for: "1week",

        // Path to the log file. If not specified, log will be written to stdout.
        // log_file: "/var/log/rammingen.conf",

        // Log filter (optional).
        // Log filter format: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html
        // log_filter: "trace",
    }
    ```
1. Restrict access to Postgres password:
    ```sh
    sudo chmod 600 /etc/rammingen-server.conf
    ```
1. Create database structure:
    ```sh
    sudo docker run \
        --volume /etc/rammingen-server.conf:/etc/rammingen-server.conf:ro \
        --entrypoint /sbin/rammingen-admin \
        --network host \
        riateche/rammingen:0.2.0-alpha.1 \
        migrate
    ```
1. Add a new source (replace `example` with desired source name):
    ```sh
    sudo docker run \
        --volume /etc/rammingen-server.conf:/etc/rammingen-server.conf:ro \
        --entrypoint /sbin/rammingen-admin \
        --network host \
        riateche/rammingen:0.2.0-alpha.1 \
        add-source example
    ```
    Save the generated access token. Repeat this step for each client.
    You should typically have one client per PC or phone.

1. Set up a systemd unit for Rammingen server. Create `/etc/systemd/system/rammingen.service` file:
    ```conf
    [Unit]
    Description=Rammingen server
    After=docker.service
    Requires=docker.service

    [Service]
    TimeoutStartSec=0
    Restart=always
    ExecStartPre=-/usr/bin/docker stop %n
    ExecStartPre=-/usr/bin/docker rm %n
    ExecStart=/usr/bin/docker run \
        --volume /etc/rammingen-server.conf:/etc/rammingen-server.conf:ro \
        --volume /var/storage:/var/storage \
        --network host \
        --rm \
        --name %n \
        riateche/rammingen:0.2.0-alpha.1

    [Install]
    WantedBy=multi-user.target
    ```
1. Start the server and enable auto-start:
    ```sh
    sudo systemctl daemon-reload
    sudo systemctl start rammingen
    sudo systemctl enable rammingen
    ```

1. Create a temporary HTTP configuration file for nginx at `/etc/nginx/sites-enabled/rammingen` (replace `example.com` with your domain name):
    ```sh
    server {
        server_name example.com;
        charset utf-8;
        listen 80;
        client_max_body_size 10G;
        location / {
            proxy_pass http://127.0.0.1:8080;
        }
    }
    ```
    Reload nginx config:
    ```sh
    sudo systemctl reload nginx
    ```
    In the next steps, we'll use certbot to enable HTTPS for the service.

    **Note: it's necessary to use an encrypted connection (HTTPS) to connect to Rammingen server.** While the file contents and metadata are always encrypted before transfer, HTTPS is still necessary to protect against MitM attacks. Connecting over plain HTTP would allow a potential attacker to do some destructive actions, such as deleting or corrupting files.

1. Install certbot:
    ```sh
    sudo apt install certbot python3-certbot-nginx
    ```
    Depending on the challenge you intend to use, you may also need to install additional packages.

1. Run certbot to obtain a certificate and reconfigure nginx:
    ```sh
    sudo certbot
    ```
    If using an alternative challenge (e.g. DNS-based), pass `--installer nginx` and the parameters for the challenge, for example:
    ```sh
    sudo certbot --installer nginx \
        --dns-standalone-address=1.2.3.4 ...
    ```
    Follow instructions in the terminal to complete the setup. Once it's complete, verify new content of the `/etc/nginx/sites-enabled/rammingen` file.

1. Set up backups for your server. **Both database and file storage should be backed up to protect against data loss.**


See [server README](server/README.md) for more information about Rammingen server.

### Client host

1. Install [rustup](https://rustup.rs/) or install `rustc` and `cargo` using a system package manager.
1. Install rammingen:
    ```sh
    cargo install --locked rammingen@0.2.0-alpha.1
    ```
1. Create and save an encryption key:

    ```sh
    rammingen generate-encryption-key
    ```
1. Run `rammingen help` to determine default config path:
    ```sh
    rammingen help
    File sync and backup utility
    Default config location: /Users/username/Library/Application Support/rammingen.conf
    ...
    ```
1. Create a configuration file at the default location:
    ```json5
    {
        // Exclude common generated and temporary files.
        always_exclude: [
            { name_equals: "target" },
            { name_equals: ".DS_Store" },
            { name_equals: ".idea" },
            { name_equals: "node_modules" },
            { name_equals: "dist" },
            { name_equals: "Thumbs.db" },
            { name_equals: "tmp" },
            { name_equals: "temp" },
            { name_equals: "storage.bin" },
            { name_matches: "^build" },
            { name_matches: "\\.bak$" },
            { name_matches: "\\.swp$" },
        ],
        // List of synchronized local paths.
        mount_points: [
            {
                // Path on the current system.
                local_path: "/Users/username/documents",
                // Path in the global virtual tree
                // shared between all clients.
                archive_path: "ar:/documents",
            },
            {
                local_path: "/Users/username/pictures",
                archive_path: "ar:/pictures",
            },
        ],
        // Your server's domain name.
        server_url: "https://example.com/",
        // Store access token and encryption key in system keyring.
        use_keyring: true,
    }
    ```

1. Run the sync and input access token and encryption key when prompted:
    ```sh
    rammingen sync
    ```
1. Configure your OS to run `rammingen auto-sync` on startup. This command will continue running in background and perform sync periodically.

### Android

See [Android app README](android/README.md) for setting up Rammingen client on an Android device.

## Caveats

- Rammingen doesn't perform diffing and partial uploads of files - if a file is changed, that entire file will be uploaded and stored, unless it's exactly the same as a file uploaded earlier.
- There is no conflict resolution. If you make conflicting changes to the same files at two devices simultaneously, the more recent change will overwrite the older one. Both versions will be available for manual selection though.
- Rammingen is not a full backup solution. When setting up the server, you are expected to set up backups for the database and the file storage that it uses.
