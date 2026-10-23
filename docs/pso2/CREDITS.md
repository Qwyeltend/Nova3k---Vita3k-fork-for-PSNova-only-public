# PSO2 in Nova3K — credits / благодарности

## Главное спасибо — Anton Maltsev

Оффлайн-игра в PSO2 в Nova3K работает на **[PhantasyServer](https://github.com/PhantasyServer/PhantasyServer)** —
сервере PSO2, который написал **Anton Maltsev** (лицензия MIT). Nova3K (автор — Qwyeltend) встраивает его копию
прямо в эмулятор (`external/phantasyserver`, изменения описаны в `NOVA3K-CHANGES.md`). Знания о протоколе — из его же
**[pso2-protocol-lib](https://github.com/PhantasyServer/pso2-protocol-lib)** (MIT / Apache-2.0).
**Без его работы это было бы невозможно. Спасибо!**

**Main thanks — Anton Maltsev.** Offline PSO2 in Nova3K (by Qwyeltend) runs on his PhantasyServer (MIT), built into
the emulator, and his pso2-protocol-lib documents the protocol. **This would not have been possible without his work.
Thank you!**

## Остальные проекты

Ресурсы и проекты, на которые опирается работа над PSO2 в Nova3K. Если не сказано иное, код из них не копировался;
использовались описания форматов и протоколов.

| Проект | Что взяли | Лицензия |
|---|---|---|
| [Vita3K](https://github.com/Vita3K/Vita3K) | Эмулятор, основа Nova3K | GPL-2.0 |
| [dynarmic](https://github.com/lioncash/dynarmic) | JIT-рекомпилятор ARM (с нашими патчами) | 0BSD |
| [psvpfstools](https://github.com/motoharu-gosuto/psvpfstools) | Расшифровка NoNpDrm-дампов при установке (с нашим патчем продолжения) | GPL-3.0 |
| [PhantasyServer](https://github.com/PhantasyServer/PhantasyServer) (Anton Maltsev) | **Код встроен в Nova3K**: сервер PSO2 для оффлайн-игры (лобби, квесты, враги, предметы) | MIT |
| [pso2-protocol-lib](https://github.com/PhantasyServer/pso2-protocol-lib) (Anton Maltsev) | Протокол PSO2, включая Vita-клиент (используется сервером) | MIT / Apache-2.0 |
| [PolarisServer](https://github.com/PolarisTeam/PolarisServer) (Polaris Team) | Описание протокола ПК-версии PSO2: формат пакетов, список кораблей (11-3D), распределение по блокам (11-2C), приветствие (03-08), обмен ключами RSA + RC4 (11-0B / 11-0C) | AGPL-3.0 (только как справочник) |
| [Arks-Layer](https://arks-layer.com/) | Английский перевод PSO2 (Vita). В Nova3K **не входит**: пользователь ставит свой файл через меню игры, Nova3K только копирует его. Также сведения о Vita-клиенте | — |

Если вы автор одного из проектов и хотите уточнить формулировку — напишите в issues.

---

Resources and projects the PSO2 work in Nova3K builds on. No code was copied from them unless stated;
we used their documentation of formats and protocols (table above).
