# Виджет для панели KDE Plasma

Отдельный плазмоид `org.lionforest.yamusicrs`: показывает, что играет, и управляет плеером
прямо с панели. Плеер трогать не нужно — виджет читает стандартный интерфейс MPRIS
(`org.mpris.MediaPlayer2.yamusic-rs`) и посылает те же команды, что медиаклавиши.

## Что умеет

- в панели: кнопка play/пауза и строка «Исполнитель — Трек»;
- по клику раскрывается попурок с кнопками «предыдущий», «play/пауза», «следующий»;
- обновляется по сигналу `PropertiesChanged` от плеера, раз в 5 секунд перечитывает
  состояние на случай пропущенного сигнала;
- если плеер не запущен, ничего не ломает: показывает пустое состояние.

## Требования

- KDE Plasma **6.0** или новее (`kpackagetool6`);
- запущенный плеер «Яндекс Музыка RS» с включённым MPRIS (включён всегда, начиная с 2.0.0).

## Установка из репозитория

```bash
kpackagetool6 --type Plasma/Applet --install extras/kde-widget
```

Обновить уже установленный:

```bash
kpackagetool6 --type Plasma/Applet --upgrade extras/kde-widget
```

Удалить:

```bash
kpackagetool6 --type Plasma/Applet --remove org.lionforest.yamusicrs
```

После установки: правый клик по панели → «Добавить элементы…» → «Яндекс Музыка RS».
Если виджет не появился в списке, перезапустите оболочку:

```bash
systemctl --user restart plasma-plasmashell
```

## Установка из пакета

Если ставили через `packaging/PKGBUILD`, виджет уже лежит в
`/usr/share/plasma/plasmoids/org.lionforest.yamusicrs` — остаётся только добавить его на панель.

## Состав

```
extras/kde-widget/
├── metadata.json          — описание плазмоида (Id: org.lionforest.yamusicrs, версия 1.0.1)
└── contents/ui/main.qml   — интерфейс и работа с MPRIS через D-Bus
```

Лицензия — GPL-3.0, как и у самого плеера.
