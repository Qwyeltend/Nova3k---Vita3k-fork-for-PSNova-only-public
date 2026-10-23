# Nova3K

**Nova3K** is an Android build of the PlayStation Vita emulator [Vita3K](https://github.com/Vita3K/Vita3K), tuned for **Phantasy Star Nova** (PCSG00351 / PCSH00143, including the English fan translation). Since 0.2.0 it also runs **Phantasy Star Online 2** for PS Vita (PCSG00141) **offline**, with a built-in server — as a demonstration (see below).

**Nova3K** — сборка эмулятора PlayStation Vita [Vita3K](https://github.com/Vita3K/Vita3K) для Android, настроенная под **Phantasy Star Nova** (PCSG00351 / PCSH00143, в том числе с английским фан-переводом). С версии 0.2.0 в нём также работает **Phantasy Star Online 2** для PS Vita (PCSG00141) — **без интернета**, со встроенным сервером. Это демонстрация (подробности ниже).

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

## Phantasy Star Online 2 (PS Vita) — offline demo / оффлайн-демо

SEGA closed the PS Vita servers of PSO2 in September 2020; without a server the game cannot get past the title screen. Nova3K starts a small PSO2 server **inside the emulator** and points the game at it, so it is played without internet.
SEGA закрыла серверы PSO2 для PS Vita в сентябре 2020 года; без сервера игра не проходит дальше титульного экрана. Nova3K запускает небольшой сервер PSO2 **внутри эмулятора** и направляет игру на него — играть можно без интернета.

**How it works / Как это сделано**
- **Server:** [PhantasyServer](https://github.com/PhantasyServer/PhantasyServer) by **Anton Maltsev** (MIT) — a PSO2 server written in Rust. Nova3K builds it for Android and runs it inside the emulator, listening only on `127.0.0.1`. Our changes are listed in `external/phantasyserver/NOVA3K-CHANGES.md`. Huge thanks to Anton Maltsev — without his server and [protocol library](https://github.com/PhantasyServer/pso2-protocol-lib) this would not exist.
  **Сервер:** [PhantasyServer](https://github.com/PhantasyServer/PhantasyServer) (автор **Anton Maltsev**, MIT) — сервер PSO2 на Rust. Nova3K собирает его под Android и запускает внутри эмулятора, он слушает только `127.0.0.1`. Наши изменения — в `external/phantasyserver/NOVA3K-CHANGES.md`. Огромное спасибо Anton Maltsev: без его сервера и [библиотеки протокола](https://github.com/PhantasyServer/pso2-protocol-lib) ничего бы не было.
- **Game patches (in memory, only for PSO2 6.31):** "Game Start" goes to login instead of the "service ended" notice; the game's server addresses resolve to the built-in server; the server's RSA key replaces SEGA's. Each patch checks the original bytes first; other versions are not touched. Nothing is written to the game files.
  **Патчи игры (в памяти, только для PSO2 6.31):** «Начать игру» ведёт ко входу, а не к сообщению о закрытии; адреса серверов игры ведут на встроенный сервер; ключ RSA сервера подменяет ключ SEGA. Каждый патч сначала проверяет исходные байты; другие версии не трогаются. Файлы игры не изменяются.
- **Emulator fixes for PSO2:** GPU downscale of the scene (60 FPS), white-screen and water fixes, a render-command fix for the character editor, PSN/network stubs the game needs. Details in [NOVA3K.md](NOVA3K.md) (Russian).
  **Исправления эмулятора для PSO2:** уменьшение сцены на видеокарте (60 FPS), исправления белого экрана и воды, исправление команд рендера для редактора персонажа, заглушки PSN/сети. Подробно — в [NOVA3K.md](NOVA3K.md).
- **Offline quests:** 15 "Exploration" quests (one per field: Forest, Caves, Desert, Tundra, Tunnels, Skyscape, Ruins, Sanctum, Coast, Quarry, Seabed, Shironia, Kuron, Tokyo, Las Vegas), generated from the game's own map data: campship → field → field → the location's boss → exit. Random field shapes, enemies of the location, level-scaled enemies and rewards.
  **Оффлайн-квесты:** 15 квестов «Exploration» (по одному на локацию), собранных из данных карт самой игры: кэмпшип → поле → поле → босс локации → выход. Случайная форма полей, местные враги, уровень врагов и награды по уровню игрока.

**What works / Что работает:** login, ship and lobby, character creation, the quest counter, the Exploration quests (gates, bosses, quest clear), combat, Monomates, death and return to the campship, 60 FPS on a Snapdragon 8 Gen 2.
**Not yet / Пока нет:** shops and most NPCs, story quests, Emergency quests, multiplayer, many items and menus. Some starts or enemies can land inside the ground (chat commands `!start`, `!next`, `!cs` help).
**Работает:** вход, корабль и лобби, создание персонажа, стойка квестов, квесты Exploration (врата, боссы, завершение), бой, Мономейты, смерть и возврат на кэмпшип, 60 FPS на Snapdragon 8 Gen 2. **Пока нет:** магазины и большинство NPC, сюжет, экстренные квесты, мультиплеер, многие предметы и меню. Старт или враги иногда оказываются в земле (помогают команды чата `!start`, `!next`, `!cs`).

**You need / Что нужно** (your own copy of the game / собственная копия игры):
| Item / Что | Details / Подробности |
|---|---|
| PSO2 for PS Vita, version 1.00 | `PCSG00141` (Japanese), with its license (zRIF / `work.bin`) |
| Update **6.31** (the final one / последнее) | the full `PCSG00141` update folder as a zip (~17 GB) — other versions are not supported / другие версии не поддерживаются |
| Free space / Место | ~25 GB internal if the update zip is on the SD card (~42 GB otherwise) |
| GPU driver / Драйвер | character models need the **Turnip** Mesa driver on Adreno (e.g. Mr. Purple T30); the system driver does not draw them / модели персонажей рисует драйвер **Turnip** |

Step-by-step installation: [docs/pso2/INSTALL.md](docs/pso2/INSTALL.md) (English below the Russian part). Credits: [docs/pso2/CREDITS.md](docs/pso2/CREDITS.md). An English fan translation by **Arks-Layer** can be installed from your own file (long-press the game → *English patch*); it is not included.
Пошаговая установка: [docs/pso2/INSTALL.md](docs/pso2/INSTALL.md). Благодарности: [docs/pso2/CREDITS.md](docs/pso2/CREDITS.md). Английский фан-перевод от **Arks-Layer** ставится из вашего файла (долгое нажатие на игру → *English patch*); в комплект не входит.

## Setup / Настройка

Same as Vita3K: install the PS Vita firmware, then install the game (a `.pkg` with its license, or a dumped game folder / `.vpk` / `.zip`). See the [Vita3K quickstart](https://vita3k.org/quickstart.html).
Как в Vita3K: установить прошивку PS Vita, затем саму игру (`.pkg` с лицензией или папку / `.vpk` / `.zip` с дампом). См. [инструкцию Vita3K](https://vita3k.org/quickstart.html).

Phantasy Star Nova needs these settings to work properly (per-game settings):
Для правильной работы Phantasy Star Nova нужны такие настройки (настройки игры):

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

The built-in PSO2 server is [PhantasyServer](https://github.com/PhantasyServer/PhantasyServer) by Anton Maltsev (MIT, see `external/phantasyserver/LICENSE`).
Встроенный сервер PSO2 — [PhantasyServer](https://github.com/PhantasyServer/PhantasyServer), автор Anton Maltsev (MIT).

Not affiliated with SEGA or Sony. Phantasy Star and Phantasy Star Online 2 are trademarks of SEGA. No game files are included or linked.
Проект не связан с SEGA и Sony. Phantasy Star и Phantasy Star Online 2 — товарные знаки SEGA. Файлы игр не входят в комплект и не распространяются.
