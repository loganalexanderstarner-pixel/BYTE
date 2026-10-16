# Laptop mode (saving battery)

Laptop mode is **off until you turn it on**: [Settings → General](byte-setting:general) → **Laptop mode**.

## What it does on battery
While your computer runs on its battery, BYTE:
- **pauses listening for "Hey BYTE"** (the microphone is not used for it),
- **pauses the screen replay** (see [Screen questions](help:screen)),
- shows a small **On battery** badge, and
- offers a one-click switch to a **smaller model** you have already downloaded, which uses less power. BYTE never downloads one for you here.

Your saved settings are not changed. When the charger is plugged in again, listening and the replay come back by themselves.

## How it decides
BYTE asks the system whether the computer is on battery or mains power every 30 seconds, and changes only after two readings in a row agree, so a flicker does not switch things off and on. A desktop computer has no battery, so nothing happens.

## Good to know
- Turning "Hey BYTE" on while on battery with laptop mode on is saved, but it stays paused until the charger is back.
- Typing and voice buttons you press yourself keep working.
