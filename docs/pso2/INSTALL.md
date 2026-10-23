# Phantasy Star Online 2 (PS Vita) в Nova3K — установка

> **Статус:** онлайн-сервисы Vita-версии закрыты SEGA 16.09.2020. Nova3K запускает внутри себя локальный сервер
> (PhantasyServer, автор Anton Maltsev): вход, корабль, лобби, создание персонажа и оффлайн-квесты «Exploration»
> (15 локаций, несколько зон, босс) работают без интернета. Это демонстрация: магазины, сюжет и многое другое
> ещё не работают.
> English version below.

Игра в комплект не входит. Ниже описано, **какие** файлы нужны и как их установить. Используйте собственную
копию игры (дамп со своей PS Vita).

## Что понадобится
| Что | Как опознать |
|---|---|
| Игра, версия 1.00 | ID `PCSG00141`, «ファンタシースターオンライン２», около 3 ГБ. zip (дамп NoNpDrm) или `.pkg` + zRIF |
| Лицензия игры | файл `.rif` / `work.bin` в дампе или строка zRIF. Content ID: `JP0177-PCSG00141_00-PHANTASYSTARONL2` |
| Обновление 6.31 (последнее) | zip около 17 ГБ: папка `PCSG00141` с `eboot.bin`, `data`, `sce_sys`, `sce_pfs`; в `sce_sys/param.sfo` `APP_VER = 06.31`, `CATEGORY = gp` |
| Прошивка PS Vita | как для любой игры в Vita3K |
| Место | ~25 ГБ свободно во внутренней памяти, если zip обновления лежит на карте памяти (иначе ~42 ГБ) |

## Шаги
1. **Прошивка.** Установите прошивку PS Vita (Install → Firmware), если ещё не стоит.
2. **Игра 1.00.** Install → Archive (ZIP/VPK) → выберите zip игры. Если у вас `.pkg`, используйте Install → Package и введите zRIF.
3. **Лицензия** (если игра не запускается или обновление не ставится): Install → License → ввести zRIF вручную.
   Должен появиться файл `ux0/license/PCSG00141/JP0177-PCSG00141_00-PHANTASYSTARONL2.rif`.
4. **Обновление 6.31.** Install → Archive → zip обновления.
   - Установка долгая (распаковка и расшифровка 17 ГБ). Держите устройство на зарядке и не сворачивайте эмулятор.
   - **Если эмулятор закрылся посреди установки — просто запустите установку того же zip ещё раз.**
     Nova3K продолжит с того места, где остановился (в логе: `resuming the install`).
5. **Проверка.** В информации об игре версия должна стать **6.31**, на титульном экране — меню «タイトルメニュー».

## Если что-то пошло не так
| Симптом | Что делать |
|---|---|
| «Уже установлено», но версия 1.00 | Удалите `Android/data/org.nova3k.emulator/files/vita/ux0/patch/PCSG00141` и `...PCSG00141_dec`, затем поставьте обновление снова |
| Установка обрывается | В логе есть строка `... MiB free on the device` — проверьте место. Затем просто повторите установку |
| zip от macOS не читается | Соберите архив в Терминале: `zip -r -0 PSO2_631.zip PCSG00141` |
| Нужен лог | `Android/data/org.nova3k.emulator/files/vita3k.log`; после вылета — `vita3k.prev.log` и `nova3k_crash.txt` |

## Английский перевод (по желанию)
Перевод на английский — работа команды **Arks-Layer** ([arks-layer.com](https://arks-layer.com/)), спасибо им.
В Nova3K он **не входит**; файл (обычно `999.cpk`) нужно взять у них. Nova3K умеет только положить ваш файл куда надо:
1. Долгое нажатие на PSO2 в списке игр → **English patch (Arks-Layer)…**
2. **Choose file…** → выберите `999.cpk`. Nova3K проверит, что это архив с текстами игры, и скопирует его в
   `ux0/app/PCSG00141/data/vita/patches/999.cpk` — игра сама загружает все архивы из этой папки.
3. Запустите игру. Вернуть японский — там же, **Remove patch**.

Известный перевод датирован 06/12/2020, а последний клиент Vita — 6.31; если какие-то строки останутся
японскими или текст будет не на месте, это расхождение версий, а не ошибка установки.

## Меню игры (перевод)
ゲームスタート — Начать игру · ユーザー情報変更 — Данные пользователя (веб) · キャラクターポートレート — Портрет персонажа (камера) ·
オフラインキャラメイク — Оффлайн-создание персонажа · 『PSO2es』との連動 — Связь с PSO2es · クレジット — Титры ·
オンラインマニュアル — Онлайн-руководство · タイトルに戻る — К титульному экрану

---

# Phantasy Star Online 2 (PS Vita) in Nova3K — installation

> **Status:** SEGA shut down the Vita online service on 2020-09-16. Nova3K runs a local server inside the emulator
> (PhantasyServer by Anton Maltsev): login, ship, lobby, character creation and offline "Exploration" quests
> (15 locations, several zones, a boss) work without internet. This is a demonstration: shops, the story and much
> more do not work yet.

The game is not included. This guide lists **which** files you need and how to install them. Use your own copy
of the game (a dump from your own PS Vita).

## You need
| Item | How to recognise it |
|---|---|
| Game, version 1.00 | ID `PCSG00141`, about 3 GB, a zip (NoNpDrm dump) or `.pkg` + zRIF |
| Game license | `.rif` / `work.bin` in the dump or a zRIF string; content ID `JP0177-PCSG00141_00-PHANTASYSTARONL2` |
| Update 6.31 (final) | ~17 GB zip: folder `PCSG00141` with `eboot.bin`, `data`, `sce_sys`, `sce_pfs`; `param.sfo` has `APP_VER = 06.31`, `CATEGORY = gp` |
| PS Vita firmware | as for any Vita3K game |
| Free space | ~25 GB internal if the update zip is on the SD card (~42 GB otherwise) |

## Steps
1. Install the firmware (Install → Firmware).
2. Install the 1.00 game zip (Install → Archive), or the `.pkg` with its zRIF (Install → Package).
3. License, if needed: Install → License → enter the zRIF.
4. Install the 6.31 update zip (Install → Archive). Keep the device charging. **If the emulator closes during
   the install, start the same install again — Nova3K resumes where it stopped.**
5. The game version should now be **6.31**.

## English patch (optional)
The English translation is the work of the **Arks-Layer** team ([arks-layer.com](https://arks-layer.com/)), thank
you! It is **not** included in Nova3K; get the file (usually `999.cpk`) from them. Nova3K only puts your file in place:
long-press PSO2 in the game list → **English patch (Arks-Layer)…** → **Choose file…**. The file is checked and copied to
`ux0/app/PCSG00141/data/vita/patches/999.cpk`, which the game loads with its own patch archives. **Remove patch**
goes back to Japanese. The known patch is dated 06/12/2020 while the last Vita client is 6.31, so some text may
stay Japanese.

## Troubleshooting
| Problem | Fix |
|---|---|
| "Already installed" but still 1.00 | Delete `.../vita/ux0/patch/PCSG00141` and `...PCSG00141_dec`, install again |
| Install stops | Check the `MiB free on the device` line in the log, free space, retry |
| macOS zip not readable | Re-create it in Terminal: `zip -r -0 PSO2_631.zip PCSG00141` |
| Logs | `vita3k.log`; after a crash `vita3k.prev.log` and `nova3k_crash.txt` |
