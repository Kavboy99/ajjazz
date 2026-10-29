# Запрос файла прошивки AK820MAX ULTRA

Английский запрос отправлен 18 октября 2026 через Gmail пользователя на `support@a-jazz.com`, указанный на странице контактов https://ajazzbrand.com/pages/contact . Gmail подтвердил «Сообщение отправлено»; открыто отправленное письмо. Индивидуальный серийный номер и вложения не отправлялись.

Письмо: https://mail.google.com/mail/u/0/#sent/FFNDWMkjTHSlpsnzFPzmtswjGtFmVNfk

Подтверждение: `artifacts/ajazz-firmware-request-sent-20261018.png`. Ниже сохранён подготовленный текст; фактическое английское письмо содержит те же идентификаторы и запросы, с приветствием AJAZZ Support и подписью Pavel.

## Русский

Тема: Офлайн-прошивка / recovery для AK820MAX ULTRA, USB 0C45:80A0

Здравствуйте. Нужен штатный офлайн-пакет прошивки или recovery для следующей клавиатуры:

- AK820MAX ULTRA, TTC Soldier King Magnetic, белая.
- EAN 6976412985287, артикул XY-MP6-AK820MAX-ULTRA.
- USB VID:PID 0C45:80A0.
- Внутренние идентификаторы: manufacturer 0x0166, product 0x1004.
- Установленная версия firmware 1.30.

Веб-драйвер ajazz.driveall.cn определяет устройство, но каталог https://ajazz.driveall.cn/update.json возвращает HTTP 200 с пустым телом. Проверка обновлений завершается ошибкой разбора JSON, хотя интерфейс сообщает, что установлена последняя версия.

Просьба предоставить BIN/HEX или офлайн-обновлятор со встроенным образом именно этой ревизии, уточнить MCU и ревизию PCB, а также процедуру восстановления с сохранением Hall-калибровки и сопряжения с приёмником. Если для приёмника требуется отдельный образ, прошу указать совместимые версии и предоставить его тоже. Подойдут также официальная прямая ссылка на файл и контрольная сумма.

## English

Subject: Offline stock firmware / recovery for AK820MAX ULTRA, USB 0C45:80A0

Hello. Please provide the stock offline firmware or recovery package for an AK820MAX ULTRA, white, TTC Soldier King Magnetic switches, EAN 6976412985287, SKU XY-MP6-AK820MAX-ULTRA.

Device identity: USB VID:PID 0C45:80A0; internal manufacturer ID 0x0166; product ID 0x1004; installed firmware 1.30.

The WebHID driver detects the keyboard, but https://ajazz.driveall.cn/update.json returns HTTP 200 with an empty body. Update checking fails with a JSON parsing error while the UI reports that the firmware is up to date.

Please send a BIN/HEX image or an offline updater containing the correct image for this revision, the MCU/PCB revision, and the recovery procedure that preserves Hall calibration and receiver pairing. If the receiver requires a separate image, please provide it and identify compatible versions. An official direct download link and checksum would also help.
