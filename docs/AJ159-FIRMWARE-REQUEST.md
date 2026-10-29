# Запрос точных образов AJ159 APEX / дока — отправлен

Отправлено 21 октября 2026 через Gmail пользователя на support@a-jazz.com. Gmail подтвердил «Сообщение отправлено». Серийный номер и вложения не отправлялись.

Письмо: https://mail.google.com/mail/u/0/#sent/QgrcJHsBpWsQLbJPJLMZwpNNJcBCTgWcNlB

Подтверждение: `artifacts/aj159-firmware-request-sent-20261021.png`.

Тема: AJ159 APEX ID1755 and dock ID1759 / OLEDV104: stock firmware and recovery package

Hello AJAZZ Support,

Please provide the stock offline firmware/recovery package for the AJ159 APEX mouse with internal device ID 1755 and its display charging dock with internal ID 1759 (USB 3151:5007, OLEDV104). The vendor radio service represents the mouse as 0x800006DB. This is the 1755/1759 revision, not the 2349/2350 variant with the same APEX name.

Fresh read-only queries on 21 October 2026 confirmed mouse ID 1755 via GET_INFOR (0x8F). GET_REV (0x80) returned bytes 80 02 03, giving raw version word 0x0302. The dock GETOLED_VERSION (0xAD) returned AD 04 01 00 00, giving OLED version 0x0104 and flash-version field 0.

The installed AJAZZ Driver (R) 2.1.94 detects the dock. On 21 October 2026, the firmware catalogue at https://api2.rongyuan.tech:3816/api/v2/get_fw_version returned HTTP 500 / Record not found for both dev_id 1755 and 1759. Please provide a direct official download link or an offline updater that contains the corresponding images, plus checksums and compatible version information for the mouse, receiver/radio, and display controllers.

Please also identify the MCU and PCB revisions, explain the recovery procedure and bootloader entry for this exact hardware, and describe how to preserve factory sensor calibration, pairing data, USB/radio functionality and charging behavior. Does this dock expose a supported USB API for per-LED RGB control or custom display widgets, or require separate display/RGB firmware?

Thank you,
Pavel
