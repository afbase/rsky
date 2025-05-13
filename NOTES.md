# Build Steps

- move compose.yaml and rsky-pds/Dockerfile to work directory
- port affinity should look like:

```docker
name: pds-localhost-deploy
version: '3.8'
services:
  postgres:
    image: postgres:17-bookworm
    container_name: rsky-postgres
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
      POSTGRES_DB: postgres
    ports:
      - "5678:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 5s
      timeout: 5s
      retries: 5
  rsky-pds:
    build:
      context: .
      dockerfile: Dockerfile
    container_name: rsky-pds
    depends_on:
      postgres:
        condition: service_healthy
    ports:
      - 8000:8000
    environment:
      - DATABASE_URL=postgres://postgres:postgres@postgres:5432/postgres
      - AWS_ENDPOINT="nyc3.digitaloceanspaces.com"
      - PDS_HOST=0.0.0.0
      - PDS_PORT=8000
      - PDS_SERVICE_HANDLE_DOMAINS="test"
      - PDS_SERVICE_DID=did:web:rsky-pds
      - AWS_EC2_METADATA_DISABLED=true
      - LOG_LEVEL=debug
      - RUST_BACKTRACE=1
    volumes:
      - rsky_data:/data
    restart: unless-stopped
volumes:
  postgres_data:
    name: rsky-postgres-data
  rsky_data:
    name: rsky-data
```

# Dependency Resolution Notes

## SQLite Dependency Conflict

There was a conflict between the `libsqlite3-sys` dependency used by different packages:

- `rsky-feedgen` and `rsky-pdsadmin` use diesel 2.1.5, which depends on `libsqlite3-sys` < 0.29.0
- `rsky-relay` uses rusqlite 0.35.0, which depends on `libsqlite3-sys` = 0.33.0

This conflict occurred because both versions of `libsqlite3-sys` attempt to link to the native SQLite library, and Cargo doesn't allow multiple packages to link to the same native library.

## Solution

The solution involved several steps:

1. Install the native SQLite development library on the system:
   ```
   sudo apt update && sudo apt install -y libsqlite3-dev
   ```

2. Update the workspace dependencies in the root Cargo.toml to provide consistent versions:
   ```toml
   diesel = { version = "2.1.5", features = ["postgres", "sqlite"] }
   ```

3. Update the rsky-pdsadmin Cargo.toml to use workspace dependencies:
   ```toml
   [dependencies]
   diesel = { workspace = true }
   # Other deps using workspace = true
   ```

4. Update the rsky-feedgen Cargo.toml to use workspace dependencies where possible:
   ```toml
   [dependencies]
   diesel = { workspace = true }
   serde = { workspace = true }
   # Other deps using workspace = true
   ```

5. Temporarily remove rsky-relay from the workspace to avoid the libsqlite3-sys conflict:
   ```toml
   members = [ "rsky-pdsadmin", "rsky-common", "rsky-crypto","rsky-feedgen", ... ]
   # Temporarily removed "rsky-relay" due to libsqlite3-sys dependency conflict
   ```

## Updated Solution with Newer Diesel

Updating to diesel 2.2.10 (from 2.1.5) was successful and all affected crates now build successfully with the newer version. The steps taken were:

1. Update workspace definition for diesel:
   ```toml
   diesel = { version = "2.2.10", features = ["postgres", "sqlite"] }
   diesel_cli = { version = "2.2.10", features = ["postgres"] }
   ```

2. Update rsky-pds to use the workspace diesel dependency:
   ```toml
   diesel = { workspace = true }
   ```

3. Update diesel_migrations in rsky-pds dev-dependencies:
   ```toml
   diesel_migrations = { workspace = true }
   ```

Despite the diesel update, the libsqlite3-sys conflict with rsky-relay remains, as the newer diesel still uses an incompatible libsqlite3-sys version compared to rusqlite.

## Future Considerations

For a more permanent solution, consider one of these options:

1. **Separate workspaces**: Split the project into two workspaces, one for rsky-relay and another for everything else.

2. **Update rusqlite**: When possible, update the rusqlite dependency in rsky-relay to be compatible with the same version of libsqlite3-sys used by diesel.

3. **Use bundled feature**: Configure both diesel and rusqlite to use the bundled SQLite feature to avoid native library conflicts.

4. **Override dependency resolutions**: Apply a more sophisticated patch or override in the workspace to force a single libsqlite3-sys version.

## Attempted Sled Update

An attempt was made to update the sled dependency in rsky-relay from revision `005c023` to the newer revision `869009a`. However, this update was unsuccessful due to significant API changes in the newer sled version:

1. The newer sled version requires extensive code changes due to:
   - Missing types that were previously used (e.g., `IVec`, `Mode`, `Error`)
   - Changed method names (e.g., `use_compression` no longer exists)
   - Different struct definitions that affect thread safety (`Sync` trait implementation issues)

2. Updating to the newer sled version would require a significant refactoring of rsky-relay's storage layer to adapt to these API changes.

## Best Solution: Make rsky-relay a Standalone Project

The most effective solution was to make rsky-relay a standalone project with its own workspace:

1. Add an empty workspace definition to rsky-relay's Cargo.toml:
   ```toml
   [workspace]
   ```

2. Update the path dependencies in rsky-relay's Cargo.toml:
   ```toml
   # internal
   rsky-common = { path = "../rsky-common" }
   rsky-identity = { path = "../rsky-identity" }
   ```

3. Remove the explicit libsqlite3-sys dependency from rsky-relay, allowing rusqlite to manage it:
   ```toml
   # Remove this line
   libsqlite3-sys = { version = "0.26.0", features = ["bundled"] }
   ```

This allows both rsky-relay and the rest of the workspace to build successfully with their own dependency resolution for sqlite.

## Building the Project

With this configuration, you can build the entire project as follows:

```bash
# Build the main workspace
cd /path/to/rsky
cargo build

# Build rsky-relay separately
cd /path/to/rsky/rsky-relay
cargo build
```