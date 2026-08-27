export type HttpStatusCategory = "1xx" | "2xx" | "3xx" | "4xx" | "5xx";

export type HttpStatusCode = {
  code: number;
  name: string;
  description: string;
  /** Когда уместно возвращать этот код. */
  usage: string;
  category: HttpStatusCategory;
};

export type HttpStatusGroup = {
  id: HttpStatusCategory;
  title: string;
  codes: HttpStatusCode[];
};

export const HTTP_STATUS_GROUPS: HttpStatusGroup[] = [
  {
    id: "1xx",
    title: "1xx — информационные",
    codes: [
      {
        code: 100,
        name: "Continue",
        description: "Сервер получил заголовки и ждёт тело запроса.",
        usage:
          "Отправляйте, когда клиент прислал Expect: 100-continue и вы готовы принять большое тело (загрузка файлов, bulk-импорт). Позволяет отклонить запрос по заголовкам до передачи мегабайтов данных. В типичном JSON REST API почти не встречается.",
        category: "1xx",
      },
      {
        code: 101,
        name: "Switching Protocols",
        description: "Сервер согласился переключиться на другой протокол.",
        usage:
          "Используйте при апгрейде соединения по заголовку Upgrade — класический пример: HTTP → WebSocket после handshake. Клиент должен был заранее запросить смену протокола; после 101 дальнейший обмен идёт уже не в формате обычного HTTP-запрос/ответ.",
        category: "1xx",
      },
      {
        code: 102,
        name: "Processing",
        description: "Запрос принят, но ответ ещё не готов.",
        usage:
          "Подходит для WebDAV и других протоколов, где операция на сервере длится заметно дольше одного RTT, но вы уже подтверждаете, что запрос принят в работу. Не заменяет 202 Accepted: здесь акцент на том, что сервер ещё обрабатывает тот же запрос, а не ставит задачу в очередь.",
        category: "1xx",
      },
      {
        code: 103,
        name: "Early Hints",
        description: "Сервер заранее отдаёт часть заголовков до финального ответа.",
        usage:
          "Применяйте на edge-серверах и CDN, чтобы браузер заранее начал preload критичных CSS, шрифтов или API до формирования полного HTML. Особенно полезно при медленном origin и server-side rendering, когда финальный ответ задерживается на сотни миллисекунд.",
        category: "1xx",
      },
    ],
  },
  {
    id: "2xx",
    title: "2xx — успех",
    codes: [
      {
        code: 200,
        name: "OK",
        description: "Запрос выполнен успешно.",
        usage:
          "Базовый успешный ответ для GET, PUT, PATCH и DELETE, когда нужно вернуть тело с данными. Для POST, создавшего ресурс, предпочтительнее 201 Created с Location. Не используйте 200 с телом ошибки — клиенты и прокси будут считать операцию успешной.",
        category: "2xx",
      },
      {
        code: 201,
        name: "Created",
        description: "Ресурс создан в результате запроса.",
        usage:
          "Возвращайте после успешного POST (или иногда PUT), когда появился новый объект: пользователь, заказ, документ. Добавьте заголовок Location с каноническим URI ресурса и по возможности тело с созданной сущностью. Если ресурс уже существовал и вы его обновили — это скорее 200 или 204.",
        category: "2xx",
      },
      {
        code: 202,
        name: "Accepted",
        description: "Запрос принят, но обработка ещё не завершена.",
        usage:
          "Выбирайте для фоновых задач: генерация отчёта, отправка письма, импорт данных. В ответе дайте ссылку на статус задачи (poll URL) или идентификатор job id. Клиент не должен считать операцию завершённой — только то, что она поставлена в очередь или запущена асинхронно.",
        category: "2xx",
      },
      {
        code: 203,
        name: "Non-Authoritative Information",
        description: "Ответ успешен, но данные могли быть изменены прокси.",
        usage:
          "Практически не используется в прикладных API. Имеет смысл только если промежуточный proxy явно трансформировал payload (сжатие, перевод, подмена зеркала) и вы хотите предупредить клиента, что это не точная копия origin. В большинстве REST-сервисов лучше отдавать обычный 200.",
        category: "2xx",
      },
      {
        code: 204,
        name: "No Content",
        description: "Запрос выполнен, тело ответа пустое.",
        usage:
          "Стандартный выбор после DELETE и для PUT/PATCH, когда клиенту не нужно обновлённое представление. Также уместен для POST-действий без создания ресурса (logout, «отметить прочитанным»). Не путайте с 404: операция прошла успешно, просто тело пустое.",
        category: "2xx",
      },
      {
        code: 205,
        name: "Reset Content",
        description: "Сервер просит клиента сбросить представление документа.",
        usage:
          "Исторически — для HTML-форм: сервер обработал запрос и просит браузер очистить поля. В современных SPA и REST API не применяется. Если нужно сообщить клиенту «обнови локальное состояние» — лучше вернуть актуальные данные в 200 или использовать события/WebSocket.",
        category: "2xx",
      },
      {
        code: 206,
        name: "Partial Content",
        description: "Возвращена только запрошенная часть ресурса.",
        usage:
          "Отвечайте на GET с заголовком Range, когда отдаёте файл или поток по частям: докачка, видеоплеер, большие бинарники. Добавьте Content-Range с диапазоном байт. Если диапазон некорректен — верните 416 Range Not Satisfiable, а не 206.",
        category: "2xx",
      },
      {
        code: 207,
        name: "Multi-Status",
        description: "В теле несколько независимых статусов операций.",
        usage:
          "Используйте в WebDAV и batch API, когда один HTTP-запрос затрагивает несколько ресурсов с разным исходом (часть успешна, часть с ошибкой). Тело обычно XML/JSON со списком пар URI + status. Для единичной операции всегда выбирайте обычный 2xx/4xx.",
        category: "2xx",
      },
      {
        code: 208,
        name: "Already Reported",
        description: "Элемент уже был перечислен в предыдущей части ответа.",
        usage:
          "Специфичен для WebDAV multistatus: избегает дублирования одного и того же binding в длинном ответе. В обычных CRUD API не нужен — если сущность уже обработана, верните 200/409 или опустите повтор в batch-результате.",
        category: "2xx",
      },
      {
        code: 226,
        name: "IM Used",
        description: "Ответ построен с учётом instance manipulations.",
        usage:
          "Редкий код для delta-encoding и IM (instance manipulations) в HTTP extensions. Применяется, когда сервер отдаёт не полный ресурс, а патч относительно версии клиента. Для обычного JSON REST и GraphQL не используется.",
        category: "2xx",
      },
    ],
  },
  {
    id: "3xx",
    title: "3xx — перенаправление",
    codes: [
      {
        code: 300,
        name: "Multiple Choices",
        description: "Есть несколько вариантов ресурса на выбор.",
        usage:
          "Когда у одного логического ресурса несколько представлений или URI и клиент должен выбрать (разные форматы, языки, версии API). На практике чаще используют content negotiation (Accept) или явные разные endpoint'ы вместо 300.",
        category: "3xx",
      },
      {
        code: 301,
        name: "Moved Permanently",
        description: "Ресурс навсегда переехал на новый URI.",
        usage:
          "Применяйте при постоянной смене URL: ребрендинг, миграция версии API, объединение маршрутов. Укажите Location на новый адрес; поисковики и клиенты могут кэшировать редирект. Для API, где важно сохранить метод POST/PUT, рассмотрите 308 вместо 301.",
        category: "3xx",
      },
      {
        code: 302,
        name: "Found",
        description: "Временный редирект на другой URI.",
        usage:
          "Исторически — «временно смотри сюда», но многие клиенты меняют POST на GET (redirect-on-post). Для API предпочтительнее явные 303 или 307/308 с предсказуемой семантикой. Оставьте 302 в основном для legacy HTML и OAuth redirect flows, где так принято.",
        category: "3xx",
      },
      {
        code: 303,
        name: "See Other",
        description: "Результат доступен по другому URI, обычно через GET.",
        usage:
          "Классический паттерн POST/Redirect/GET: после создания или действия перенаправьте браузер GET-запросом на страницу результата, чтобы refresh не повторил POST. В REST для машинных клиентов чаще возвращают 201 с Location, без редиректа.",
        category: "3xx",
      },
      {
        code: 304,
        name: "Not Modified",
        description: "Ресурс не изменился с момента последней проверки.",
        usage:
          "Ответ на условный GET с If-None-Match (ETag) или If-Modified-Since, когда версия на сервере не новее. Экономит трафик и ускоряет клиентов с локальным кэшем. Тело не передаётся; клиент использует сохранённую копию. Не путайте с 204 — здесь именно «кэш актуален».",
        category: "3xx",
      },
      {
        code: 305,
        name: "Use Proxy",
        description: "Доступ к ресурсу возможен только через указанный прокси.",
        usage:
          "Устарел и не должен использоваться в новых системах из соображений безопасности (proxy injection). Современные сети настраивают прокси через PAC, VPN или системные политики, а не через HTTP 305.",
        category: "3xx",
      },
      {
        code: 306,
        name: "Switch Proxy",
        description: "Больше не используется.",
        usage:
          "Зарезервирован и снят с практики. Не возвращайте этот код — ни один современный клиент не должен на него полагаться.",
        category: "3xx",
      },
      {
        code: 307,
        name: "Temporary Redirect",
        description: "Временный редирект с сохранением метода запроса.",
        usage:
          "Временно направьте клиента на другой URI, сохранив метод и тело (POST остаётся POST). Подходит для maintenance redirect, A/B маршрутизации, временного failover. Клиент должен повторять запросы на исходный URI после снятия редиректа.",
        category: "3xx",
      },
      {
        code: 308,
        name: "Permanent Redirect",
        description: "Постоянный редирект с сохранением метода запроса.",
        usage:
          "Как 301, но без неявной смены POST → GET. Предпочтителен при постоянном переносе API endpoint'ов, если клиенты шлют не только GET. Location указывает новый канонический URI; кэширование редиректа допустимо.",
        category: "3xx",
      },
    ],
  },
  {
    id: "4xx",
    title: "4xx — ошибка клиента",
    codes: [
      {
        code: 400,
        name: "Bad Request",
        description: "Запрос некорректен или не может быть обработан.",
        usage:
          "Первый выбор при ошибке на стороне клиента: битый JSON, неверный тип поля, отсутствие обязательного параметра, невалидный query. В теле опишите, что именно не так (поле, код ошибки). Не используйте 400 для «нет прав» (403) или «не найдено» (404) — это путает интеграторов.",
        category: "4xx",
      },
      {
        code: 401,
        name: "Unauthorized",
        description: "Требуется аутентификация или она не прошла.",
        usage:
          "Когда credentials отсутствуют, просрочены или неверны: нет Bearer-токена, истёк JWT, неверный пароль. Добавьте WWW-Authenticate с hint, как авторизоваться. Если пользователь уже залогинен, но не имеет прав — это 403, не 401.",
        category: "4xx",
      },
      {
        code: 402,
        name: "Payment Required",
        description: "Зарезервирован для оплаты; в стандарте почти не определён.",
        usage:
          "Иногда применяют для paywall, исчерпанной квоты тарифа или необходимости привязать карту. Семантика не стандартизирована — зафиксируйте её в документации API. Альтернатива: 403 с кодом ошибки subscription_required или 429 при лимитах.",
        category: "4xx",
      },
      {
        code: 403,
        name: "Forbidden",
        description: "Доступ запрещён, даже если клиент аутентифицирован.",
        usage:
          "Пользователь известен, но действие запрещено: нет роли admin, доступ к чужому ресурсу, IP в blacklist, feature flag выключен. Не раскрывайте лишние детали о существовании ресурса, если это вопрос безопасности. Отличайте от 401: здесь повторная авторизация не поможет.",
        category: "4xx",
      },
      {
        code: 404,
        name: "Not Found",
        description: "Ресурс не найден.",
        usage:
          "Неизвестный id в URL, удалённая запись, опечатка в path, скрытый endpoint для неавторизованных (security through obscurity — спорно). Для коллекций иногда возвращают пустой список с 200, а 404 — когда искали конкретный ресурс по id. Не используйте 404 вместо 403, если хотите явно сказать «нет доступа».",
        category: "4xx",
      },
      {
        code: 405,
        name: "Method Not Allowed",
        description: "HTTP-метод не поддерживается для этого URI.",
        usage:
          "DELETE на read-only ресурсе, PATCH там, где разрешён только GET. Обязательно верните заголовок Allow со списком допустимых методов. Помогает клиентам и генераторам SDK быстро понять контракт endpoint'а.",
        category: "4xx",
      },
      {
        code: 406,
        name: "Not Acceptable",
        description: "Сервер не может вернуть формат из Accept.",
        usage:
          "Клиент запросил application/xml или image/png, а API умеет только application/json. Реже — несовместимые Accept-Language. Если можете — всё же отдайте дефолтный формат с 200; 406 уместен, когда альтернативы нет или клиент явно исключил все поддерживаемые типы.",
        category: "4xx",
      },
      {
        code: 407,
        name: "Proxy Authentication Required",
        description: "Нужна аутентификация на прокси.",
        usage:
          "Клиент обращается через HTTP-proxy, которому нужны отдельные credentials (типично корпоративные сети). Не путайте с 401 origin-сервера. В прикладных REST API за reverse proxy этот код обычно отдаёт сам proxy, а не ваше приложение.",
        category: "4xx",
      },
      {
        code: 408,
        name: "Request Timeout",
        description: "Сервер не дождался полного запроса от клиента.",
        usage:
          "Клиент слишком медленно отправляет тело или обрывает соединение до конца заголовков. Отличается от 504: здесь виноват медленный клиент, а не upstream. На уровне приложения чаще ограничивают размер/время через 413 или обрывают на load balancer.",
        category: "4xx",
      },
      {
        code: 409,
        name: "Conflict",
        description: "Конфликт с текущим состоянием ресурса.",
        usage:
          "Дубликат unique-поля (email уже занят), параллельное редактирование одной версии, нарушение state machine (нельзя отменить уже shipped заказ). Хорошо сочетается с телом ошибки, описывающим конфликт. Для простой валидации полей предпочтительнее 422.",
        category: "4xx",
      },
      {
        code: 410,
        name: "Gone",
        description: "Ресурс удалён навсегда и не вернётся.",
        usage:
          "Сильнее 404: ресурс был, но намеренно и permanently удалён (deprecated API version, GDPR erasure). Клиенты могут перестать повторять запросы. Если ресурс мог появиться позже — оставьте 404.",
        category: "4xx",
      },
      {
        code: 411,
        name: "Length Required",
        description: "Нужен заголовок Content-Length.",
        usage:
          "Сервер требует явный размер тела и не принимает chunked без Length — редкость в современных стеках. Может встретиться на строгих proxy или legacy upload endpoint'ах. Обычно проще принять Transfer-Encoding: chunked.",
        category: "4xx",
      },
      {
        code: 412,
        name: "Precondition Failed",
        description: "Не выполнено условие из If-Match или If-Unmodified-Since.",
        usage:
          "Оптимистичная блокировка: клиент прислал устаревший ETag, ресурс уже изменили. Клиент должен перечитать актуальную версию и повторить update. Стандартный паттерн для collaborative editing и PATCH/PUT с версионированием.",
        category: "4xx",
      },
      {
        code: 413,
        name: "Payload Too Large",
        description: "Тело запроса слишком большое.",
        usage:
          "Превышен лимит upload (файл, JSON, batch). Укажите в документации максимальный размер; при возможности верните понятное сообщение. Отличается от 414 (длинный URL) и 431 (огромные заголовки). На gateway часто настраивают до попадания в приложение.",
        category: "4xx",
      },
      {
        code: 414,
        name: "URI Too Long",
        description: "URI слишком длинный для обработки.",
        usage:
          "Слишком длинный query string (тысячи фильтров, base64 в URL). Решение для клиента — перенести параметры в тело POST или сократить запрос. Серверы и nginx по умолчанию имеют лимит ~8K; 414 сигнализирует о его превышении.",
        category: "4xx",
      },
      {
        code: 415,
        name: "Unsupported Media Type",
        description: "Неподдерживаемый Content-Type.",
        usage:
          "Клиент прислал text/plain или multipart там, где API принимает только application/json, или наоборот. В ошибке укажите поддерживаемые типы. Частая ошибка интеграции — забытый Content-Type: application/json при POST.",
        category: "4xx",
      },
      {
        code: 416,
        name: "Range Not Satisfiable",
        description: "Запрошенный диапазон байт недоступен.",
        usage:
          "Ответ на некорректный Range: диапазон за пределами файла, перепутанные границы. Добавьте Content-Range: */length, чтобы клиент понял допустимый размер. При валидном Range возвращайте 206 Partial Content.",
        category: "4xx",
      },
      {
        code: 417,
        name: "Expectation Failed",
        description: "Сервер не может выполнить условие из Expect.",
        usage:
          "Почти не встречается: сервер не поддерживает Expect: 100-continue или не может удовлетворить указанное ожидание. Если не реализуете 100 Continue — можно отклонять такие запросы; многие серверы просто игнорируют Expect.",
        category: "4xx",
      },
      {
        code: 418,
        name: "I'm a teapot",
        description: "Шуточный код: «чайник не варит кофе».",
        usage:
          "RFC 2324, пасхалка: «я чайник и не могу заварить кофе». Допустим в demo API, April Fools, healthcheck с юмором. Никогда не используйте в production для реальных ошибок — клиенты и мониторинг не поймут серьёзность.",
        category: "4xx",
      },
      {
        code: 421,
        name: "Misdirected Request",
        description: "Запрос отправлен на сервер, который не обслуживает этот ресурс.",
        usage:
          "Типично для HTTP/2 и shared TLS: запрос попал на сервер, который не обслуживает данный :authority или SNI. Исправление — правильный Host/SNI и маршрутизация на ingress. Из приложения обычно возвращает front proxy, не бизнес-логика.",
        category: "4xx",
      },
      {
        code: 422,
        name: "Unprocessable Entity",
        description: "Синтаксис верный, но семантика данных неверна.",
        usage:
          "JSON распарсился, но бизнес-валидация не прошла: дата в прошлом, сумма отрицательная, несовместимые поля. Стандарт de facto для форм и REST (Rails, OpenAPI). Отличайте от 400 (синтаксис) и 409 (конфликт состояния). Возвращайте список ошибок по полям.",
        category: "4xx",
      },
      {
        code: 423,
        name: "Locked",
        description: "Ресурс заблокирован.",
        usage:
          "WebDAV и редактирование документов: файл заблокирован другим пользователем или checkout. Клиенту нужно дождаться снятия блокировки или запросить force unlock. В обычном CRUD без lock-механизма чаще используют 409 Conflict.",
        category: "4xx",
      },
      {
        code: 424,
        name: "Failed Dependency",
        description: "Операция зависит от другого запроса, который завершился ошибкой.",
        usage:
          "WebDAV batch: «не удалось переместить B, потому что не удалось заблокировать A». В каскадных API — когда шаг pipeline зависел от предыдущего failed step. Для одиночных операций верните конкретную 4xx/5xx причины, а не 424.",
        category: "4xx",
      },
      {
        code: 425,
        name: "Too Early",
        description: "Сервер не готов обработать запрос из-за риска replay-атаки.",
        usage:
          "Защита при TLS 1.3 0-RTT early data: запрос пришёл слишком рано, его могли перехватить и повторить. Клиент должен повторить без early data. Настраивается на edge; прикладной код редко возвращает 425 вручную.",
        category: "4xx",
      },
      {
        code: 426,
        name: "Upgrade Required",
        description: "Клиенту нужно перейти на другой протокол.",
        usage:
          "Требуйте HTTPS вместо HTTP, TLS 1.2+, WebSocket после upgrade или новую версию API. Добавьте Upgrade и Connection в ответ. Полезно для принудительного шифрования и deprecated plain-text endpoint'ов.",
        category: "4xx",
      },
      {
        code: 428,
        name: "Precondition Required",
        description: "Запрос должен быть условным (If-Match и т.п.).",
        usage:
          "Сервер требует If-Match/If-Unmodified-Since на изменяющих методах, чтобы клиент не перезаписывал чужие изменения вслепую. Разумная политика для публичных API с конкурентными PATCH. Без precondition — отклоняйте с 428, а не молча затирайте данные.",
        category: "4xx",
      },
      {
        code: 429,
        name: "Too Many Requests",
        description: "Превышен лимит частоты запросов.",
        usage:
          "Rate limiting по IP, API key или user id. Добавьте Retry-After (секунды или HTTP-date) и заголовки X-RateLimit-* при наличии. Клиент должен backoff и повторить позже. Не путайте с 503: здесь клиент злоупотребляет, а сервер в целом жив.",
        category: "4xx",
      },
      {
        code: 431,
        name: "Request Header Fields Too Large",
        description: "Заголовки запроса слишком большие.",
        usage:
          "Огромные Cookie, JWT в Authorization, кастомные заголовки выше лимита nginx/Node. Решение — сократить cookie, вынести данные в тело, использовать session id вместо fat token. Часто возникает при неочищенных dev-cookie на localhost.",
        category: "4xx",
      },
      {
        code: 451,
        name: "Unavailable For Legal Reasons",
        description: "Доступ запрещён по юридическим причинам.",
        usage:
          "Блокировка по судебному решению, GDPR takedown, geo-restriction по закону, DMCA. Можно добавить ссылку на policy в теле или Link. Прозрачнее, чем маскировать под 404, когда юридически нужно явно отказать.",
        category: "4xx",
      },
    ],
  },
  {
    id: "5xx",
    title: "5xx — ошибка сервера",
    codes: [
      {
        code: 500,
        name: "Internal Server Error",
        description: "Непредвиденная ошибка на стороне сервера.",
        usage:
          "Fallback для необработанных исключений, багов, падения БД без graceful handling. Логируйте stack trace и correlation id; клиенту — generic message без утечки internals. Если ошибка предсказуема (timeout БД) — лучше 503/504. Не возвращайте 500 для ошибок валидации — это 4xx.",
        category: "5xx",
      },
      {
        code: 501,
        name: "Not Implemented",
        description: "Метод или возможность сервером не реализованы.",
        usage:
          "Endpoint задокументирован, но ещё не shipped; метод PATCH не поддерживается этой версией; feature flag выключен навсегда. Отличие от 405: здесь capability отсутствует глобально, а не запрещена для конкретного URI. Можно добавить Retry-After, если реализация скоро появится.",
        category: "5xx",
      },
      {
        code: 502,
        name: "Bad Gateway",
        description: "Шлюз получил некорректный ответ от upstream-сервера.",
        usage:
          "Reverse proxy, API gateway или BFF получил от бэкенда битый ответ, connection reset или HTML вместо JSON. Типично при деплое, crash pod'а, misconfigured upstream. Клиент может retry с backoff; мониторинг должен алертить на рост 502.",
        category: "5xx",
      },
      {
        code: 503,
        name: "Service Unavailable",
        description: "Сервис временно недоступен.",
        usage:
          "Плановое обслуживание, перегрузка, circuit breaker открыт, graceful shutdown. Добавьте Retry-After. Клиент должен повторить позже, а не считать запрос невалидным. Отличие от 502: сервис сознательно недоступен или перегружен, а не «upstream сломался неожиданно».",
        category: "5xx",
      },
      {
        code: 504,
        name: "Gateway Timeout",
        description: "Шлюз не дождался ответа от upstream-сервера.",
        usage:
          "Timeout между nginx/Envoy и медленным микросервисом, долгим SQL или внешним API. Upstream ещё может доработать задачу — идempotency важна при retry. Увеличьте timeout на gateway или оптимизируйте бэкенд; для длинных job лучше 202 Accepted.",
        category: "5xx",
      },
      {
        code: 505,
        name: "HTTP Version Not Supported",
        description: "Версия HTTP не поддерживается.",
        usage:
          "Клиент говорит на HTTP/1.0 или экспериментальной версии, а сервер принимает только HTTP/1.1 или HTTP/2. Редко встречается в practice; чаще соединение обрывается на TLS/ALPN. Ответьте 505, если парсер протокола явно отвергает версию.",
        category: "5xx",
      },
      {
        code: 506,
        name: "Variant Also Negotiates",
        description: "Ошибка прозрачного согласования контента.",
        usage:
          "Misconfiguration content negotiation: вариант ответа ссылается сам на себя или образует цикл. Исправляется настройкой сервера, а не клиентским retry. Прикладной код API practically never returns 506.",
        category: "5xx",
      },
      {
        code: 507,
        name: "Insufficient Storage",
        description: "Недостаточно места для выполнения операции.",
        usage:
          "WebDAV COPY/MOVE, upload, когда на томе закончился disk quota. Также возможно при лимите bucket/object storage. Клиенту нужно освободить место или выбрать другой target; retry без изменений бессмысленен.",
        category: "5xx",
      },
      {
        code: 508,
        name: "Loop Detected",
        description: "Обнаружен бесконечный цикл при обработке.",
        usage:
          "WebDAV: копирование каталога в самого себя, циклические symlink. Остановите операцию до stack overflow. В HTTP API без иерархических copy — practically unused.",
        category: "5xx",
      },
      {
        code: 510,
        name: "Not Extended",
        description: "Для запроса нужны дополнительные расширения.",
        usage:
          "Клиент не прислал обязательный extension-заголовок, без которого сервер не может выполнить RFC-расширение. Крайне редко; в custom API проще вернуть 400 с текстом «нужен заголовок X».",
        category: "5xx",
      },
      {
        code: 511,
        name: "Network Authentication Required",
        description: "Нужна сетевая аутентификация для доступа в интернет.",
        usage:
          "Captive portal в Wi‑Fi (кафе, аэропорт, hotel): перед доступом в интернет нужно войти или принять terms. Браузеры распознают 511 и показывают login page. Не используйте в обычном REST API — это про сетевой доступ, не про ваш Bearer token.",
        category: "5xx",
      },
    ],
  },
];

export const HTTP_STATUS_CODES: HttpStatusCode[] = HTTP_STATUS_GROUPS.flatMap(
  (group) => group.codes,
);

export function findHttpStatusCode(code: number): HttpStatusCode | undefined {
  return HTTP_STATUS_CODES.find((entry) => entry.code === code);
}
