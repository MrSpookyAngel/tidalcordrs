# TidalCordRS

**TidalCordRS** is a Discord music bot built in Rust.
It's the sequel to [TidalCord](https://github.com/MrSpookyAngel/TidalCord).
It allows you to retrieve music from Tidal and play it in your Discord server.
**Tidal Premium account required.**

---

## Disclaimer

TidalCordRS might go against the terms of service of Tidal.
By using this software, you accept full responsibility for any potential issues, including account bans or other consequences.
The creator of this project is not responsible for any misuse or violations of third-party terms of service.

---

## Retrieve your Discord Token

1. Visit the [Discord developer portal](https://discord.com/developers/applications)
2. Create a new application and enter whatever you want to name your bot
3. Go to the `Bot` tab
   * Your current URL should look like this: `https://discord.com/developers/applications/<bot-id>/bot`
4. Under the `Privileged Gateway Intents` section, enable the following intents:
   * `Server Members Intent`
   * `Message Content Intent`
5. Remember to click `Save Changes`
6. Click `Reset Token` and take note of your token
   * This token should be added to your `.env` as `DISCORD_TOKEN`

## Add the Discord Bot to your Server

1. Visit the [Discord developer portal](https://discord.com/developers/applications)
2. Click on your bot application
3. Go to the `OAuth2` tab
4. Under the `Client Information` section and take note of your `Client ID`
5. Copy and paste this URL into your browser: `https://discord.com/oauth2/authorize?client_id=<your-client-id>&permissions=36776960&integration_type=0&scope=bot`
   * Replace `<your-client-id>` with your actual `Client ID`
6. You should now be prompted to add the bot to one of your servers

## How to Run

0. Retrieve your Discord Token and add the bot your server
1. Install [FFmpeg](https://ffmpeg.org/) to your system PATH
   * If you're struggling with this, then search for "FFmpeg system PATH <your operating system\>" online for more information
2. Create an empty folder on your computer
3. Download a [release](https://github.com/MrSpookyAngel/tidalcordrs/releases) and place it inside the empty folder
4. Download the [`example.env`](https://github.com/MrSpookyAngel/tidalcordrs/blob/main/example.env) and place it inside the empty folder
   * Rename `example.env` to `.env`
5. Update the `.env` as needed, but only the `DISCORD_TOKEN` is required to update from the default
6. Run the program
7. Visit the Tidal link shown in the console and authorize the device
   * After authorizing, the console will say that your bot is connected
8. Check out the [wiki](https://github.com/MrSpookyAngel/tidalcordrs/wiki) to view available commands and their usage
---

