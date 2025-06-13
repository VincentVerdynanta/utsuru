# Getting started

This guide contains a basic tutorial on how you can get your way around
utsuru.

## Installation

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of utsuru running in the terminal][terminal-image]

</details>

Once the utsuru executable is installed on your system, open a terminal and run:

- on Windows: `utsuru.exe`
- on Linux and macOS: `./utsuru`

Upon execution, it will print a Web UI URL to the terminal. Open this URL in a browser to access the utsuru Web UI.

[terminal-image]: https://github.com/user-attachments/assets/da835365-36d1-48aa-9306-c2803122ef33

## Configuring OBS Output Settings

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of OBS Output Settings][output-settings-image]

</details>

In this two consecutive sections, we will guide you through configuring OBS (Open Broadcaster Software) with our recommended **Output** and **Video** settings. While you're not forced to use these settings, we recommend them as they were tested during utsuru's development.

Once you have OBS open, go to the **Settings** menu and navigate to the **Output** tab. In this section, you'll need to adjust both:

1. the **Video Bitrate** to **2500 Kbps**.
2. the **Audio Bitrate** to **160**.

These settings are designed to achieve the optimal balance between quality and performance during streaming, but you can adjust them based on your own preferences or network conditions.

[output-settings-image]: https://github.com/user-attachments/assets/29922382-b58a-400f-ad71-5577859469a8

## Configuring OBS Video Settings

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of OBS Video Settings][video-settings-image]

</details>

Next, go to the **Video** tab in OBS settings. Here,

1. adjust the **Output Resolution** to 1280x720.
2. set the **FPS Value** to 30.

These settings help maintain a stable and smooth stream without overwhelming your system resources.

[video-settings-image]: https://github.com/user-attachments/assets/20e4295c-9c36-4276-a46a-287c2c5aa662

## Setting up OBS for Streaming

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of OBS Stream Settings][stream-settings-image]

</details>

Now, head to the **Stream** tab in OBS settings. Set the **Service** to WHIP. This will allow OBS to connect to the utsuru service for streaming.

[stream-settings-image]: https://github.com/user-attachments/assets/441b27ac-edae-4895-913e-6eb547f431fe

## Connecting OBS to utsuru

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of utsuru's Web UI][webui-image]

</details>

Once you’ve adjusted the **Service**, go back to the utsuru Web UI. The interface will display two sections: **Mirrors** and **WHIP**. In the WHIP section, there will be a WHIP server URL and bearer token. Copy both values and return to OBS. In the OBS settings, paste both the WHIP server URL and bearer token into the fields under **Destination**.

At this point, OBS will prompt you with a message stating that changing the Service to WHIP will change the audio encoder to Opus. Click **Yes** to continue. After confirming, apply the changes and close the settings window. You are now ready to start streaming!

[webui-image]: https://github.com/user-attachments/assets/033d631c-4d14-41a3-8020-0532e52dd248

## Setting up Discord Live Mirror

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of utsuru's Add Mirror form][add-mirror-image]

</details>

Once OBS is streaming, return to the utsuru Web UI. Under the **Mirrors** section, click the **+** button at the top to create a new Discord Live connection. A popup form will appear, prompting you for your Discord token, guild ID, and voice channel ID.

[add-mirror-image]: https://github.com/user-attachments/assets/9b89a41a-bcef-4dc9-8879-b9d25ab89a55

## Retrieving Discord Token

> [!CAUTION]
> utsuru will not work without a Discord **user account** (as opposed to a **bot account**). This is because Discord does not allow bot accounts to **Go Live**. Since utsuru connects as a user account, using it will be similar to using Discord with a custom client, which is against Discord’s Terms of Service.
>
> We want to emphasize that **we do not take responsibility** for the standing status of your Discord account. As such, we highly recommend signing up for a new Discord account that is dedicated solely to streaming with utsuru.

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of Discord client's DevTools][discord-devtools-image]

</details>

To proceed, you will need your Discord token. Open Discord and locate your token. If you're unfamiliar with this process, you may need to search online (using Google or YouTube) for guidance on retrieving your Discord token. Once you have your token, copy it and return to the utsuru Web UI. Paste the token into the first input field in the popup form.

[discord-devtools-image]: https://github.com/user-attachments/assets/e39511d2-2b86-41a8-8cc2-2d1f5c315fcb

## Retrieving Discord Guild ID and Voice Channel ID

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of Discord client's voice channel page][discord-vc-image]

</details>

Next, go back to Discord and navigate to the guild and voice channel where you want utsuru to connect. Right-click on the desired voice channel and select **Copy Link**. Paste the copied link into a text box, such as the one in a Discord text channel or simply Notepad. The link will be in the format:

```text
https://discord.com/channels/GUILD_ID/CHANNEL_ID
```

From this URL, copy the value of the **GUILD\_ID** (the first numbers in the link) and return to the utsuru Web UI. Paste this value into the second input field of the popup form. For example, from the link:

```text
https://discord.com/channels/41771983423143937/127121515262115840
```

The **GUILD\_ID** would be `41771983423143937`.

Next, copy the **CHANNEL\_ID** (the second numbers in the link) and paste it into the third input field. For example, from the same link, the **CHANNEL\_ID** would be `127121515262115840`.

[discord-vc-image]: https://github.com/user-attachments/assets/3ed84a9b-2b59-4c2a-9242-e8961214883b

## Finalizing the Discord Live Mirror

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of utsuru's Web UI with a Mirror entry][mirror-entry-image]

</details>

Once you've entered the Discord **token**, **guild ID**, and **voice channel ID**, click the **+ Add** button in the popup form to begin the connection process. If the connection is successful, the popup will automatically close, and a new entry will appear in the **Mirrors** section of the utsuru Web UI.

You have now successfully connected utsuru to your Discord voice channel and can begin streaming to it.

[mirror-entry-image]: https://github.com/user-attachments/assets/02114e98-66bf-4582-971f-6ba957cfb5fe
