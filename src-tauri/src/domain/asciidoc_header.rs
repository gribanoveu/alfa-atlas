//! Правило «шапка документа оторвана от заголовка».
//!
//! Шапка AsciiDoc заканчивается первой пустой строкой (спецификация,
//! docs.asciidoctor.org/asciidoc/latest/document/header). Атрибуты после неё —
//! уже атрибуты тела, и `:toc:` из тела не действует: оглавление не строится
//! вовсе. Ловушка тихая — `:sectnums:` из тела при этом продолжает работать,
//! сами строки атрибутов в текст не выводятся, а проверка стандарта K.2.1
//! ищет подстроку `:toc:` и такую шапку пропускает.
//!
//! Чистая функция: никакого I/O и никакого asciidoctor — проверяется только
//! взаимное расположение строк.

/// Ищет атрибут, оторванный от шапки: документ начинается с заголовка `= …`,
/// затем идёт пустая строка, а после неё — строка вида `:имя:`.
///
/// Возвращает номер строки (1-based) первого такого атрибута. `None`, если
/// заголовка документа нет, шапка не разорвана или оторванных атрибутов нет.
///
/// Проверяются только атрибуты, отделённые от шапки пустыми строками и
/// комментариями: `:name:` глубоко в теле документа — законный приём
/// (переопределить значение по ходу текста), и на него правило не срабатывает.
pub fn detached_header_attribute_line(content: &str) -> Option<u32> {
    let lines: Vec<&str> = content.lines().collect();
    let title = document_title_line(&lines)?;

    // Пройти остаток шапки: атрибуты и комментарии сразу за заголовком.
    let mut i = title + 1;
    while i < lines.len() && (is_attribute_entry(lines[i]) || is_comment(lines[i])) {
        i += 1;
    }
    // Шапка обязана закончиться пустой строкой — иначе там просто начался текст.
    if i >= lines.len() || !lines[i].trim().is_empty() {
        return None;
    }
    // За пустыми строками и комментариями должен идти хотя бы один атрибут —
    // это и есть оторванная часть шапки.
    while i < lines.len() && (lines[i].trim().is_empty() || is_comment(lines[i])) {
        i += 1;
    }
    if i < lines.len() && is_attribute_entry(lines[i]) {
        return Some(i as u32 + 1);
    }
    None
}

/// Строка заголовка документа (`= Название`) с учётом предшествующих
/// комментариев и пустых строк, которые шапку не открывают.
fn document_title_line(lines: &[&str]) -> Option<usize> {
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start_matches('\u{feff}');
        if trimmed.trim().is_empty() || is_comment(trimmed) {
            continue;
        }
        return trimmed.starts_with("= ").then_some(i);
    }
    None
}

/// `:name:` или `:name: value`, в том числе `:name!:` (снятие атрибута).
fn is_attribute_entry(line: &str) -> bool {
    let rest = match line.strip_prefix(':') {
        Some(rest) => rest,
        None => return false,
    };
    let Some(end) = rest.find(':') else {
        return false;
    };
    let name = &rest[..end];
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '!')
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_the_attribute_block_pushed_out_of_the_header() {
        let content = "= Rest-метод createX\n\n:sectnums:\n:toc: left\n\n== Назначение\n";
        assert_eq!(detached_header_attribute_line(content), Some(3));
    }

    #[test]
    fn a_well_formed_header_is_silent() {
        let content = "= Rest-метод createX\n:sectnums:\n:toc: left\n\n== Назначение\n";
        assert_eq!(detached_header_attribute_line(content), None);
    }

    #[test]
    fn comments_around_the_title_do_not_confuse_it() {
        // Комментарии перед заголовком шапку не открывают и не ломают.
        let ok = "// образец\n= Rest-метод createX\n:toc: left\n\n== Назначение\n";
        assert_eq!(detached_header_attribute_line(ok), None);
        let broken = "// образец\n= Rest-метод createX\n\n// пояснение\n:toc: left\n\n== Назначение\n";
        assert_eq!(detached_header_attribute_line(broken), Some(5));
    }

    #[test]
    fn an_attribute_redefined_inside_the_body_is_not_a_detached_header() {
        let content =
            "= Rest-метод createX\n:toc: left\n\n== Назначение\nТекст.\n\n:sectnums:\n\n== Алгоритм\n";
        assert_eq!(detached_header_attribute_line(content), None);
    }

    #[test]
    fn documents_without_a_title_are_out_of_scope() {
        // Фрагменты для include — у них своей шапки нет.
        let content = "== Раздел\n\n:attribute: value\n";
        assert_eq!(detached_header_attribute_line(content), None);
    }

    #[test]
    fn a_header_that_runs_straight_into_text_is_not_flagged() {
        let content = "= Rest-метод createX\n:toc: left\nТекст сразу после шапки.\n";
        assert_eq!(detached_header_attribute_line(content), None);
    }

    #[test]
    fn title_with_a_byte_order_mark_is_still_a_title() {
        let content = "\u{feff}= Rest-метод createX\n\n:toc: left\n\n== Назначение\n";
        assert_eq!(detached_header_attribute_line(content), Some(3));
    }
}
