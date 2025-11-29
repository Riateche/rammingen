# Rammingen server

Rammingen server handles database and file storage and provides API to the clients. **Both database and file storage should be backed up to protect against data loss.**

An instance of rammingen server can only serve one user and provides them with full access to all stored files and metadata. Multi-user support may be implemented later.

The user can run `rammingen` clients on all their systems. These clients should be connected to the same rammingen server. It's recommended to create a separate `source` for each system where you'd like to run the client. You can assign a separate name and access token to each of the sources. In the version history, the name of the source will be displayed for every change of an entry.

Rammingen uses end-to-end encryption to store file content and metadata on the server. All file content and metadata (including file names) is encrypted before it's sent to the server. To decrypt it, an encryption key is required. The encryption key should only be accessible by rammingen clients and should never leave their systems. (All clients must use the same encryption key.)

## Pre-requisites

The following instructions will be given for Linux. However, Windows and MacOS are also supported.

Before setting up rammingen-server on your server, the following steps are recommended:

1. Obtain a domain name that will be used to access the service and set up a DNS record to point to your server.
2. Install Nginx.
3. Install PostgreSQL server. (Note: it's also possible to configure rammingen-server to use a remote database.)
4. Create a database and a user that can access it.

## Setting up

It's recommended to use docker or podman to run rammingen-server on Linux. Replace `docker` with `podman` in the commands if you use podman.

Check [https://hub.docker.com/r/riateche/rammingen-server/tags](Dockerhub page) to get the latest Docker image version.

1. Create a configuration file. Default path for the configuration file on Linux is `/etc/rammingen-server.conf`, but it can be changed using `--config` command line option. The configuration file uses [JSON5](https://json5.org/) format.

    Here's an example of a valid configuration file:
    ```json5
    {
        // URL of the database.
        "database_url": "postgres://dbuser:dbpassword@dbhostname:dbport/dbname",
        // Path to the local file storage.
        "storage_path": "/var/storage",
        // IP and port that the server will listen.
        "bind_addr": "127.0.0.1:8080",
        // Time between snapshots. A snapshot is a copy of the state
        // of all archive entries at a certain time.
        // Snapshots are not deleted automatically.
        // Supported duration formats: https://docs.rs/humantime/latest/humantime/fn.parse_duration.html
        "snapshot_interval": "1week",
        // Time during which all recorded entry versions are stored in the database.
        // Entry versions that are older than `retain_detailed_history_for` will
        // eventually be deleted, except for entry versions that are part of a snapshot.
        "retain_detailed_history_for": "1week",

        // Path to the log file. If not specified, log will be written to stdout.
        // "log_file": "/var/log/rammingen.conf",

        // Log filter (optional).
        // Log filter format: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html
        // "log_filter": "trace",
    }
    ```

2. Create the required database structure:
    ```sh
    docker run \
        --volume /etc/rammingen-server.conf:/etc/rammingen-server.conf:ro \
        --entrypoint /sbin/rammingen-admin \
        riateche/rammingen:0.1.4 migrate
    ```

3. Add a new source:
    ```sh
    docker run \
        --volume /etc/rammingen-server.conf:/etc/rammingen-server.conf:ro \
        --entrypoint /sbin/rammingen-admin \
        riateche/rammingen:0.1.4 add-source homepc
    ```
    Take note of the access token printed by the command. Add this token to the client config.

    Repeat for every client you'd like to configure.

4. Run the server once to verify the configuration:
    ```sh
    docker run \
        --volume /etc/rammingen-server.conf:/etc/rammingen-server.conf:ro \
        --volume /var/storage/:/var/storage/ \
        riateche/rammingen:0.1.4
    ```
5. Consult documentation for docker/podman and your operating system to set up rammingen-server as a continuously running service, for example, by creating a new systemd service for it. It's also recommended to run it under a non-root user.

6. Create a nginx host configuration. If you'd like to use `certbot --nginx` later, create a temporary non-SSL configuration first:
    ```ini
    # in /etc/nginx/sites-enabled/rammingen-server
    server {
        server_name myhostname.example;
        charset utf-8;
        listen 80 http2;

        # Allow uploading large files.
        client_max_body_size 10G;

        location / {
            proxy_pass http://127.0.0.1:8080;
        }
    }
    ```
7. Obtain a SSL certificate for your domain. Using `certbot` is recommended.

    **Note: it's necessary to use an encrypted connection (HTTPS) to connect to rammingen-server.** While the data being transfered is encrypted, the rammingen protocol itself is not designed to be resistant against MitM attacks. Connecting over plain HTTP would allow a potential attacker to do some destructive actions, such as deleting or corrupting files.

    Final nginx config with SSL can look like this:
    ```ini
    # in /etc/nginx/sites-enabled/rammingen-server
    server {
        server_name myhostname.example;
        charset utf-8;

        listen 443 ssl http2;
        ssl_certificate /etc/letsencrypt/live/myhostname.example/fullchain.pem;
        ssl_certificate_key /etc/letsencrypt/live/myhostname.example/privkey.pem;
        # verify chain of trust of OCSP response using Root CA and Intermediate certs
        ssl_trusted_certificate /etc/letsencrypt/live/myhostname.example/chain.pem;
        include /etc/nginx/ssl.conf;

        # Allow uploading large files.
        client_max_body_size 10G;

        location / {
            proxy_pass http://127.0.0.1:8080;
        }
    }
    ```

## Building from source

Install [rustup](https://rustup.rs/).

To build rammingen-server, run the following commands:
```sh
git clone git@github.com:Riateche/rammingen.git
cd rammingen
cargo build --release --package rammingen_server
```
Output binaries (`rammingen-server` and `rammingen-admin`) will be created in `target/release`.

## Updating sqlx query metadata

If any SQL queries are added or modified in the code, the build will fail until the sqlx files in `server/.sqlx` are updated.

First, install `sqlx-cli` if it's not installed yet:
```sh
cargo install sqlx-cli@0.8.6
```
Next, run a temporary database in Docker:
```sh
docker run --name rammingen_local \
    -e POSTGRES_HOST_AUTH_METHOD=trust \
    -p 6123:5432 \
    -d \
    postgres:alpine
```
Apply migrations to the database:
```sh
cargo run -p rammingen_server --bin rammingen-admin -- \
    --database-url postgres://postgres@127.0.0.1:6123/ \
    migrate
```
Update SQLX files:
```sh
export DATABASE_URL=postgres://postgres@127.0.0.1:6123/
cd server
cargo sqlx prepare
```
The files in `server/.sqlx` will be updated.

## Virtual file systems and database structure

All user files from all sources are arranged in a virtual filesystem tree. Files and directories within that tree are identified by **archive path** (`ar:/...`). Archive paths are mapped to local paths in rammingen client config. Archive paths are not accessible on server. Instead, each archive path is converted to an encrypted representation - an **encrypted archive path** (`enar:/...`). A notable property of this representation is that it preserves parent-child relationship of paths. That allows the server to maintain important invariants and handle some high-level commands (e.g. move or remove a directory recursively) without knowing any real file names or directories.

The server stores the file metadata as entries and entry versions. An **entry** (stored in `entries` table) is the current state of a particular encrypted path in the virtual archive filesystem. (That means that it also corresponds to a certain non-encrypted archive path, but that path is not available to the server.) An entry can represent a directory, a file, or an absense of a previously existing directory or file. The directory's entry doesn't include its content - that is stored as separate entries. For any entry, all parent paths (up to the archive root) must correspond to an existing directory entry.

Every time an entry is created or updated, an **entry version** is created with the new properties of the entry. A list of entry versions corresponding to a certain entry represents a history of changes of the entry at a particular path. Entry versions are created using a PostgreSQL trigger.

## Snapshots and version history

Rammingen server stores all recent versions of all files (based on `retain_detailed_history_for` config option), so that any changes can be rolled back or inspected. However, it also provides a mechanism for cleaning up older information about versions.

Every once in a while (based on `snapshot_interval` config option), the server will create a snapshot of the current state of the virtual filesystem tree. A snapshot can be thought of as a lightweight copy of the state of all archive entries at a certain time.

For example, if the server config has `snapshot_interval: "1week", retain_detailed_history_for: "2weeks"`, the available history will be as follows:

* From distant past to 2 weeks ago - snapshots only (e.g. file versions at 2 weeks ago, 3 weeks ago, 4 weeks ago, etc.)
* From 2 weeks ago until now - all file versions.

Removing detailed history only happens when a snapshot is created, so actual time interval for which the detailed history is available may be larger than the configured value of `retain_detailed_history_for`.

Note that the list of changes for a file or directory will only display snapshots that actually contain changes compared to a previous snapshot.
