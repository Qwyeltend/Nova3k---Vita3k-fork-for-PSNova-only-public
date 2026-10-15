# Nova3K

**Nova3K** is an Android build of the PlayStation Vita emulator [Vita3K](https://github.com/Vita3K/Vita3K), tuned for a single game: **Phantasy Star Nova** (PCSG00351 / PCSH00143, including the English fan translation).

**Nova3K** — сборка эмулятора PlayStation Vita [Vita3K](https://github.com/Vita3K/Vita3K) для Android, настроенная под одну игру: **Phantasy Star Nova** (PCSG00351 / PCSH00143, в том числе с английским фан-переводом).

> The game is **not** included. You need your own dump of the game.
> Игра в комплект **не входит**: нужен собственный дамп игры.

## Download / Скачать

APK files are on the **[Releases](../../releases)** page. Android 9+ on arm64 (64-bit) devices.
APK — на странице **[Releases](../../releases)**. Android 9+, устройства arm64 (64-бит).

Nova3K installs next to the regular Vita3K and does not replace it.
Nova3K ставится рядом с обычным Vita3K и не заменяет его.

## What is different from Vita3K / Чем отличается от Vita3K

- **Performance / Производительность:** a lock-free exclusive monitor in the dynarmic JIT. The game's engine spins on `LDREX`/`STREX` locks, and in stock dynarmic every such instruction on every thread went through one global lock. On a Snapdragon 8 Gen 2 this took the game from 35–45 to 55–60 FPS (with the 60 FPS patch), with no audio stutter.
  Монитор эксклюзивного доступа в JIT без глобальной блокировки. Движок игры крутит блокировки на `LDREX`/`STREX`, а в обычном dynarmic каждая такая инструкция любого потока шла через один общий замок. На Snapdragon 8 Gen 2 это подняло игру с 35–45 до 55–60 FPS (с патчем 60 FPS) и убрало заикание звука.
- **No black screen / Нет чёрного экрана:** fixes the hang before the SEGA logo that newer Vita3K builds have with this game.
  Исправлено зависание перед логотипом SEGA, которое есть у новых сборок Vita3K с этой игрой.
- **Character textures / Текстуры персонажа:** fixes for broken skin, hair and face textures in the character editor (GPU-surface copies are now completed and synchronized before use).
  Исправлены сломанные текстуры кожи, волос и лица в редакторе персонажа (копирование с GPU-поверхностей теперь завершается и синхронизируется до использования).
- **Built-in 60 FPS patch / Встроенный патч 60 FPS:** enable **FPS Hack** in the game's settings. The game uses variable timestep, so it does not run faster.
  Включается галочкой **FPS Hack** в настройках игры. Игра считает время по реальным кадрам, поэтому не ускоряется.
- Smoother frame pacing on 90/120 Hz screens (FIFO presentation, 60 Hz display request). The **V-Sync** option is also available for Vulkan.
  Ровнее показ кадров на экранах 90/120 Гц (режим FIFO, запрос 60 Гц у системы). Галочка **V-Sync** доступна и для Vulkan.
- Shader logs and texture dumps go to `<emulator folder>/nova3k/`, which a file manager can open.
  Логи шейдеров и дампы текстур пишутся в `<папка эмулятора>/nova3k/`, доступную из файлового менеджера.

A detailed technical change list (in Russian) is in [NOVA3K.md](NOVA3K.md).
Подробный технический список изменений — в [NOVA3K.md](NOVA3K.md).

## Setup / Настройка

Same as Vita3K: install the PS Vita firmware, then install the game (a `.pkg` with its license, or a dumped game folder / `.vpk` / `.zip`). See the [Vita3K quickstart](https://vita3k.org/quickstart.html).
Как в Vita3K: установить прошивку PS Vita, затем саму игру (`.pkg` с лицензией или папку / `.vpk` / `.zip` с дампом). См. [инструкцию Vita3K](https://vita3k.org/quickstart.html).

The game needs these settings to work properly (per-game settings):
Для правильной работы игре нужны такие настройки (настройки игры):

| Setting / Настройка | Value / Значение |
|---|---|
| GPU / Рендер | **Vulkan** |
| V-Sync | **ON / вкл** |
| Disable surface sync | **OFF / выкл** |
| Async Pipeline Compilation | **ON / вкл** |
| Memory mapping / Отображение памяти | **Double buffer** |
| FPS Hack | **ON / вкл** (for 60 FPS / для 60 FPS) |
| Audio / Звук | **SDL** |

## Building / Сборка

GitHub Actions → **Nova3K Android** → **Run workflow**. A version number publishes a release. The workflow needs two repository secrets: `SIGNING_STORE_BASE64` (base64 of the keystore) and `SIGNING_STORE_PASSWORD`.
GitHub Actions → **Nova3K Android** → **Run workflow**. С номером версии сборка публикуется как релиз. Нужны два секрета репозитория: `SIGNING_STORE_BASE64` (keystore в base64) и `SIGNING_STORE_PASSWORD`.

## License / Лицензия

GPL-2.0, like Vita3K (see [COPYING.txt](COPYING.txt)). All credit for the emulator itself goes to the [Vita3K team](https://github.com/Vita3K/Vita3K) and to the authors of [dynarmic](https://github.com/lioncash/dynarmic).
GPL-2.0, как у Vita3K. Вся заслуга за сам эмулятор — у [команды Vita3K](https://github.com/Vita3K/Vita3K) и авторов [dynarmic](https://github.com/lioncash/dynarmic).

Not affiliated with SEGA or Sony. Phantasy Star is a trademark of SEGA.
Проект не связан с SEGA и Sony. Phantasy Star — товарный знак SEGA.
