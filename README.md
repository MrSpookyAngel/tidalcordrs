# TidalCordRS

TidalCordRS is a Discord music bot for playing music from Tidal in a Discord voice channel. It is written in Rust and supports both slash commands, like `/play`, and prefix commands, like `!play`.

This is the Rust sequel to [TidalCord](https://github.com/MrSpookyAngel/TidalCord).

> [!IMPORTANT]
> A Tidal Premium account is required.

## Before You Start

You will need:

- A Discord server where you can add bots
- A Discord bot token
- A Tidal Premium account
- FFmpeg installed, unless you run the bot with Docker
- A downloaded release, Docker, or a local Rust toolchain

## Terms Notice

TidalCordRS may violate Tidal's terms of service. By using this project, you accept responsibility for any consequences, including account restrictions or bans. The project creator is not responsible for misuse or third-party terms-of-service violations.

## Quick Start

1. Create a Discord bot and copy its token.
2. Invite the bot to your Discord server.
3. Copy `example.env` to `.env`.
4. Put your Discord bot token in `.env`.
5. Start TidalCordRS.
6. Open the Tidal authorization link printed in the console.
7. Join a voice channel and try `/play <song name>`.

The detailed steps below walk through each part.

## Create a Discord Bot

1. Open the [Discord developer portal](https://discord.com/developers/applications).
2. Select **New Application**.
3. Give the application a name, then create it.
4. Open the **Bot** tab.
5. Under **Privileged Gateway Intents**, enable:
   - **Server Members Intent**
   - **Message Content Intent**
6. Select **Save Changes**.
7. Select **Reset Token** and copy the new token.

Keep this token private. Anyone with the token can control your bot.

## Invite the Bot to Your Server

1. In the Discord developer portal, open your application.
2. Open the **OAuth2** tab.
3. Copy the **Client ID**.
4. Open this URL in your browser, replacing `<your-client-id>` with the Client ID:

   ```text
   https://discord.com/oauth2/authorize?client_id=<your-client-id>&permissions=36776960&integration_type=0&scope=bot%20applications.commands
   ```

5. Choose the server you want to add the bot to.

## Configure the Bot

Create a `.env` file beside the bot executable or beside `docker-compose.yml`.
Use `.env` for secrets and runtime values. Most bot behavior can stay on the
built-in defaults, live in `config.toml`, or be overridden with environment
variables.

The easiest starting point is to copy the example file:

```sh
cp example.env .env
```

Then edit `.env` and set your Discord token:

```env
DISCORD_TOKEN="paste-your-discord-bot-token-here"
```

TidalCordRS loads configuration in this order:

1. Built-in defaults.
2. `config.toml`, when present or when `CONFIG_PATH` is set.
3. Environment variable overrides.

That means a normal home-server setup can use a readable TOML file, while
Portainer, Docker, Kubernetes, and other deployment tools can override specific
settings without editing that file.

Common environment settings:

| Setting | Default | What it does |
| --- | --- | --- |
| `DISCORD_TOKEN` | Required | Your Discord bot token. |
| `CONFIG_PATH` | Optional | Path to a TOML config file. If unset, `config.toml` in the current directory is used when it exists. |
| `TZ` | `UTC` | Timezone used in logs, such as `America/Los_Angeles`. |
| `TIDALCORDRS_COMMAND_PREFIX` | `!` | Prefix for text commands, such as `!play`. |
| `TIDALCORDRS_BOT_PROFILE__SYNC_ENABLED` | `true` | Updates the bot profile name and avatar on startup. |
| `TIDALCORDRS_BOT_PROFILE__NAME` | `TidalCordRS` | Optional bot display name used when profile sync is enabled. |
| `TIDALCORDRS_BOT_PROFILE__AVATAR_PATH` | Built-in avatar | Optional path to a custom avatar image. |

Most users only need to change `DISCORD_TOKEN`.

Legacy environment variables such as `COMMAND_PREFIX`, `BOT_PROFILE_SYNC_ENABLED`,
`BOT_NAME`, and `BOT_AVATAR_PATH` are still supported.

### Optional TOML Config

For a readable config file, copy the example:

```sh
cp config.example.toml config.toml
```

Then edit `config.toml`:

```toml
command_prefix = "!"

[bot_profile]
sync_enabled = true
name = "TidalCordRS"
avatar_path = ""
state_path = "data/bot_profile_state.json"

[technical]
spool_read_ahead_mib = 16
collection_track_fetch_concurrency = 8
```

Environment variables still override values from `config.toml`. Nested TOML keys
use double underscores in environment variables:

| TOML setting | Environment override | Legacy override |
| --- | --- | --- |
| `command_prefix` | `TIDALCORDRS_COMMAND_PREFIX` | `COMMAND_PREFIX` |
| `bot_profile.sync_enabled` | `TIDALCORDRS_BOT_PROFILE__SYNC_ENABLED` | `BOT_PROFILE_SYNC_ENABLED` |
| `bot_profile.name` | `TIDALCORDRS_BOT_PROFILE__NAME` | `BOT_NAME` |
| `bot_profile.avatar_path` | `TIDALCORDRS_BOT_PROFILE__AVATAR_PATH` | `BOT_AVATAR_PATH` |
| `bot_profile.state_path` | `TIDALCORDRS_BOT_PROFILE__STATE_PATH` | `BOT_PROFILE_STATE_PATH` |
| `technical.spool_read_ahead_mib` | `TIDALCORDRS_TECHNICAL__SPOOL_READ_AHEAD_MIB` | `SPOOL_READ_AHEAD_MIB` |
| `technical.collection_track_fetch_concurrency` | `TIDALCORDRS_TECHNICAL__COLLECTION_TRACK_FETCH_CONCURRENCY` | `COLLECTION_TRACK_FETCH_CONCURRENCY` |
| `tidal.token_session_path` | `TIDALCORDRS_TIDAL__TOKEN_SESSION_PATH` | `TIDAL_TOKEN_SESSION_PATH` |
| `tidal.client_id` | `TIDALCORDRS_TIDAL__CLIENT_ID` | `TIDAL_CLIENT_ID` |
| `tidal.client_secret` | `TIDALCORDRS_TIDAL__CLIENT_SECRET` | `TIDAL_CLIENT_SECRET` |
| `tidal.user_agent` | `TIDALCORDRS_TIDAL__USER_AGENT` | `USER_AGENT` |
| `tidal.oauth_device_auth_url` | `TIDALCORDRS_TIDAL__OAUTH_DEVICE_AUTH_URL` | `OAUTH_DEVICE_AUTH_URL` |
| `tidal.oauth_token_url` | `TIDALCORDRS_TIDAL__OAUTH_TOKEN_URL` | `OAUTH_TOKEN_URL` |
| `tidal.sessions_url` | `TIDALCORDRS_TIDAL__SESSIONS_URL` | `SESSIONS_URL` |
| `tidal.search_url` | `TIDALCORDRS_TIDAL__SEARCH_URL` | `SEARCH_URL` |

## Run From a Release

Use this option if you just want to run the bot.

1. Install [FFmpeg](https://ffmpeg.org/) and make sure it is available in your system `PATH`.
2. Create a folder for the bot.
3. Download the latest release from the [releases page](https://github.com/MrSpookyAngel/tidalcordrs/releases).
4. Put the release executable and `.env` file in the same folder.
5. Start the executable:

   ```sh
   ./tidalcordrs
   ```

   On Windows, run:

   ```powershell
   .\tidalcordrs.exe
   ```

6. Follow the Tidal authorization link shown in the console.

When authorization succeeds, the console will report that the bot is connected.

## Run With Docker

Docker includes FFmpeg, so you do not need to install FFmpeg separately.

1. Copy `example.env` to `.env`.
2. Set `DISCORD_TOKEN` in `.env`.
3. Start the container:

   ```sh
   docker compose up -d
   ```

4. View the startup logs:

   ```sh
   docker compose logs -f tidalcordrs
   ```

5. Open the Tidal authorization link shown in the logs.

TidalCordRS stores its Tidal session and profile state in the Docker volume named `app-data`.

### Docker With TOML

To use `config.toml` with Docker, uncomment the config volume in
`docker-compose.yml` and set `CONFIG_PATH`:

```yaml
services:
  tidalcordrs:
    environment:
      - DISCORD_TOKEN=${DISCORD_TOKEN:?DISCORD_TOKEN is required}
      - CONFIG_PATH=/app/config/config.toml
    volumes:
      - ./config.toml:/app/config/config.toml:ro
      - app-data:/app/data
```

### Portainer Git Deployments

For Portainer Git deployments, deploy the repository's `docker-compose.yml` and
set variables in Portainer's stack environment section. This lets you keep the
compose file synced with Git while still changing per-install settings.

Minimum Portainer variables:

```env
DISCORD_TOKEN=paste-your-discord-bot-token-here
```

Optional overrides:

```env
TIDALCORDRS_COMMAND_PREFIX=?
TIDALCORDRS_BOT_PROFILE__NAME=My Music Bot
TIDALCORDRS_BOT_PROFILE__SYNC_ENABLED=false
```

Any setting in the override table above can be set the same way in Portainer.

Portainer users can also use TOML by mounting a config file on the host and
setting `CONFIG_PATH=/app/config/config.toml`, but environment overrides are the
simplest path for Git-based stacks.

## Build From Source

Use this option if you want to develop or run directly from the repository.

1. Install Rust.
2. Install FFmpeg and make sure it is available in your system `PATH`.
3. Copy `example.env` to `.env`.
4. Set `DISCORD_TOKEN` in `.env`.
5. Run:

   ```sh
   cargo run --release
   ```

## First Commands to Try

Join a Discord voice channel, then use either slash commands or prefix commands:

```text
/play Tennyson You
/queue
/skip
/stop
/help
```

With the default prefix, the same commands also work like this:

```text
!play Tennyson You
!queue
!skip
!stop
!help
```

For the full command list, use `/help` in Discord or visit the [wiki](https://github.com/MrSpookyAngel/tidalcordrs/wiki).

## Troubleshooting

**The bot starts, but commands do not work.**

Make sure **Message Content Intent** is enabled in the Discord developer portal. Slash commands are registered when the bot connects to your server.

**Slash commands do not appear in Discord.**

Invite the bot again with the URL above. The invite URL must include the `applications.commands` scope.

**The bot cannot play audio.**

If you are not using Docker, make sure FFmpeg is installed and available in your system `PATH`.

**The bot never joins voice.**

Make sure the bot has permission to view channels, connect to voice channels, and speak in voice channels.

**The Tidal authorization link expired.**

Stop and restart the bot, then use the new authorization link printed in the console.

**The bot profile changes on startup.**

Set this in `.env` if you do not want TidalCordRS to update the bot name or avatar:

```env
TIDALCORDRS_BOT_PROFILE__SYNC_ENABLED="false"
```
