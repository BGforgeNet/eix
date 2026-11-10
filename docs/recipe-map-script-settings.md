# Mod settings for map scripts

A mod can give players a setting in its own INI file and read it in its map scripts, through eix's
[INI variables](engine.md#ini-variables). Without eix the setting reads 0, so 0 should mean the game's own behavior.

1. Ship the INI file in the mod's folder, with the default value:

   ```ini
   ; mymod.ini
   [Companions]
   Travel=0
   ```

2. Read it in a map script with the game's `GSGetVar`, naming `ini.<file>.<section>.<key>`:

   ```
   if
   (
     IsEqual( GSGetVar( 0, "ini.mymod.Companions.Travel" ) , 1 )
   )
   then
   (
     ...
   )
   ```

3. Players change the value in `mymod.ini` and restart the game.

Notes:

- A `mymod.ini` in the game folder serves when the mod's folder has none.
- A value is a number; `0.5` and `0,5` both read as a half. A missing file, section or key reads 0.
- The value is not stored in the game: changing the INI and restarting changes it in every save.
- The conditions of an `if` must all hold. For "either of two" conditions with values 0 or 1, write
  `Not( IsGreater( Not( a ) , b ) )`, or give each condition its own `if` with the same commands.
