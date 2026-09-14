# Rough Idea: Multiplayer Bot

Captured 2026-09-13 from the maintainer's request, verbatim intent preserved.

## The idea

A new feature called "Multiplayer Bot" which allows the user to play a 2P versus
session against a computer-controlled bot.

- The user should be able to select the difficulty level of the bot on a 1–10 scale,
  10 being the hardest.
- The difficulty determines how closely to perfect the bot actually plays:
  - 10 ⇒ there is a chance the bot gets a perfect MFC.
  - 1 ⇒ there is a decent likelihood of the bot failing the song (but not guaranteed).

## Proposed shape (maintainer's initial sketch)

- Many of the building blocks should already be in place.
- A new option injected into the game's 9-options menu (and under Player Options in
  the 0-0-0 overlay menu) labeled **"Bot Opponent (1P Only)"** — similar to the
  center-arrows 1P-only option. When enabled, a new child row is shown letting the
  user select the bot's difficulty scale.
- Once in-game, the game should temporarily think we're playing a 2P session instead
  of a 1P session, with the other player computer-controlled.
- The effect should last through the results screen, so the comparison can be seen on
  the results screen after playing against the bot.

## Control mechanism sketch

- Utilize the game's autoplay mode for the bot side, but use the difficulty to inject
  some amount of jitter into the steps so it is not a guaranteed perfect play every
  time.
