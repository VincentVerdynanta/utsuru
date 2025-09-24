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

[terminal-image]: https://github.com/user-attachments/assets/4bbbd3b4-64f1-40c9-a881-52b727089219

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

[output-settings-image]: https://github.com/user-attachments/assets/facf4fa5-48b0-49f9-a949-4bf19972a6e0

## Configuring OBS Video Settings

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of OBS Video Settings][video-settings-image]

</details>

Next, go to the **Video** tab in OBS settings. Here,

1. adjust the **Output Resolution** to 1280x720.
2. set the **FPS Value** to 30.

These settings help maintain a stable and smooth stream without overwhelming your system resources.

[video-settings-image]: https://github.com/user-attachments/assets/82da1954-9c9f-4f93-b8d4-e9a18d1e0503

## Setting up OBS for Streaming

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of OBS Stream Settings][stream-settings-image]

</details>

Now, head to the **Stream** tab in OBS settings. Set the **Service** to WHIP. This will allow OBS to connect to the utsuru service for streaming.

[stream-settings-image]: https://github.com/user-attachments/assets/10916656-4ed6-4f95-a6d6-e2b0a416fda2

## Connecting OBS to utsuru

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of utsuru's Web UI][webui-image]

</details>

Once you’ve adjusted the **Service**, go back to the utsuru Web UI. The interface will display two sections: **Mirrors** and **WHIP**. In the WHIP section, there will be a WHIP server URL and bearer token. Copy both values and return to OBS. In the OBS settings, paste both the WHIP server URL and bearer token into the fields under **Destination**.

At this point, OBS will prompt you with a message stating that changing the Service to WHIP will change the audio encoder to Opus. Click **Yes** to continue. After confirming, apply the changes and close the settings window. You are now ready to start streaming!

[webui-image]: https://github.com/user-attachments/assets/4947517a-3c39-48dc-a108-eba3d9b25785

## Setting up Discord Live Mirror

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of utsuru's Add Mirror form][add-mirror-image]

</details>

Once OBS is streaming, return to the utsuru Web UI. Under the **Mirrors** section, click the **+** button at the top to create a new Discord Live connection. A popup form will appear, prompting you for your Discord token, guild ID, and voice channel ID.

[add-mirror-image]: https://github.com/user-attachments/assets/ba072722-7c5e-4d2f-bb4e-c97d8f86f4d5

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

[discord-devtools-image]: https://github.com/user-attachments/assets/36e0f431-462b-44cd-bb03-7b2817b56024

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

[discord-vc-image]: https://github.com/user-attachments/assets/6c4247d0-0f7b-41d7-a3de-d2f9e4b07940

## Finalizing the Discord Live Mirror

<details>

<summary><i>🖼️ Expand to view screenshot.</i></summary>

![A screenshot of utsuru's Web UI with a Mirror entry][mirror-entry-image]

</details>

Once you've entered the Discord **token**, **guild ID**, and **voice channel ID**, click the **+ Add** button in the popup form to begin the connection process. If the connection is successful, the popup will automatically close, and a new entry will appear in the **Mirrors** section of the utsuru Web UI.

You have now successfully connected utsuru to your Discord voice channel and can begin streaming to it.

[mirror-entry-image]: https://github.com/user-attachments/assets/cd5cfb1a-cc45-478f-84d4-619a04414bd0
