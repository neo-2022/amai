use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Date, Duration, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset, Weekday};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatQuestionInterpretation {
    pub intent: String,
    pub chat_reference: Option<String>,
    pub at_time_rfc3339: Option<String>,
    pub messages_count: usize,
    pub include_chat_messages: bool,
}

pub fn current_local_now() -> OffsetDateTime {
    let now_utc = OffsetDateTime::now_utc();
    match UtcOffset::current_local_offset() {
        Ok(offset) => now_utc.to_offset(offset),
        Err(_) => now_utc,
    }
}

pub fn interpret(question: &str, now: OffsetDateTime) -> Option<ChatQuestionInterpretation> {
    let trimmed = question.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = normalize_question(trimmed);
    let messages_count = extract_messages_count(&normalized).unwrap_or(2);

    if let Some(target) = parse_time_anchor(trimmed, &normalized, now) {
        return Some(ChatQuestionInterpretation {
            intent: "chat_at_time".to_string(),
            chat_reference: None,
            at_time_rfc3339: target.format(&Rfc3339).ok(),
            messages_count,
            include_chat_messages: true,
        });
    }

    let chat_reference = detect_chat_reference(&normalized);
    let continuity_like = is_continuity_question(&normalized);
    if chat_reference.is_none() && !continuity_like {
        return None;
    }

    let intent = match chat_reference.as_deref() {
        Some(reference) if reference == "previous" || reference.starts_with("previous:") => {
            "previous_chat"
        }
        _ => "last_chat",
    }
    .to_string();
    let include_chat_messages = chat_reference.is_some() || mentions_messages(&normalized);

    Some(ChatQuestionInterpretation {
        intent,
        chat_reference,
        at_time_rfc3339: None,
        messages_count,
        include_chat_messages,
    })
}

fn normalize_question(question: &str) -> String {
    question.to_lowercase().replace('ё', "е")
}

fn tokenize_words(question: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in question.chars() {
        if ch.is_alphanumeric() || ('а'..='я').contains(&ch) {
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn trim_datetime_token(token: &str) -> &str {
    token.trim_matches(|ch: char| {
        !(ch.is_ascii_alphanumeric()
            || ch == '-'
            || ch == ':'
            || ch == '.'
            || ch == '/'
            || ch == '+'
            || ch == 'T'
            || ch == 'Z'
            || ch == 't'
            || ch == 'z')
    })
}

fn detect_chat_reference(question: &str) -> Option<String> {
    let mentions_chat = question.contains("чат");
    if mentions_chat && question.contains("позапрош") {
        return Some("previous:2".to_string());
    }
    if mentions_chat
        && let Some(offset) = extract_chats_ago_offset(question)
        && offset >= 1
    {
        return Some(format!("previous:{offset}"));
    }
    if mentions_chat
        && (question.contains("текущ")
            || question.contains("этот чат")
            || question.contains("этом чате")
            || question.contains("current chat"))
    {
        return Some("current".to_string());
    }
    if mentions_chat
        && (question.contains("прошл")
            || question.contains("предыдущ")
            || question.contains("последний чат")
            || question.contains("последнем чате")
            || question.contains("last chat"))
    {
        return Some("previous".to_string());
    }
    None
}

fn is_continuity_question(question: &str) -> bool {
    question.contains("на чем останов")
        || question.contains("на чем закончил")
        || question.contains("на чём останов")
        || question.contains("на чём закончил")
        || question.contains("о чем говорили")
        || question.contains("о чём говорили")
        || question.contains("что было в чате")
}

fn mentions_messages(question: &str) -> bool {
    question.contains("сообщени") || question.contains("message")
}

fn extract_messages_count(question: &str) -> Option<usize> {
    if question.contains("последнее сообщение") || question.contains("last message")
    {
        return Some(1);
    }

    let tokens = tokenize_words(question);
    for (index, token) in tokens.iter().enumerate() {
        if !token.starts_with("сообщ") && token != "messages" && token != "message" {
            continue;
        }
        for candidate in tokens[..index].iter().rev().take(3) {
            if let Ok(value) = candidate.parse::<usize>() {
                return Some(value);
            }
            if let Some(value) = parse_small_number_word(candidate) {
                return Some(value);
            }
        }
    }
    None
}

fn parse_small_number_word(token: &str) -> Option<usize> {
    match token {
        "один" | "одно" | "одну" | "one" => Some(1),
        "два" | "две" | "two" => Some(2),
        "три" | "three" => Some(3),
        "четыре" | "four" => Some(4),
        "пять" | "five" => Some(5),
        "шесть" | "six" => Some(6),
        "семь" | "seven" => Some(7),
        "восемь" | "eight" => Some(8),
        "девять" | "nine" => Some(9),
        "десять" | "ten" => Some(10),
        _ => None,
    }
}

fn parse_time_anchor(
    raw_question: &str,
    normalized_question: &str,
    now: OffsetDateTime,
) -> Option<OffsetDateTime> {
    if let Some(value) = parse_explicit_datetime_tokens(raw_question, now.offset()) {
        return Some(value);
    }
    if let Some(value) = parse_relative_datetime(normalized_question, now) {
        return Some(value);
    }
    None
}

fn parse_explicit_datetime_tokens(raw_question: &str, offset: UtcOffset) -> Option<OffsetDateTime> {
    let tokens: Vec<&str> = raw_question
        .split_whitespace()
        .map(trim_datetime_token)
        .filter(|token| !token.is_empty())
        .collect();

    for token in &tokens {
        if let Ok(value) = OffsetDateTime::parse(token, &Rfc3339) {
            return Some(value);
        }
    }

    for window in tokens.windows(2) {
        let candidate = format!("{} {}", window[0], window[1]);
        if let Some(value) = parse_explicit_local_datetime(&candidate, offset) {
            return Some(value);
        }
    }

    for token in &tokens {
        if let Some(date) = parse_explicit_date(token) {
            let time = extract_time_of_day(raw_question).unwrap_or(Time::MIDNIGHT);
            return Some(date.with_time(time).assume_offset(offset));
        }
    }

    None
}

fn parse_explicit_local_datetime(candidate: &str, offset: UtcOffset) -> Option<OffsetDateTime> {
    for format in [
        format_description!("[year]-[month]-[day] [hour]:[minute]"),
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
        format_description!("[year]-[month]-[day]T[hour]:[minute]"),
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]"),
        format_description!("[day].[month].[year] [hour]:[minute]"),
        format_description!("[day].[month].[year] [hour]:[minute]:[second]"),
        format_description!("[year]/[month]/[day] [hour]:[minute]"),
        format_description!("[year]/[month]/[day] [hour]:[minute]:[second]"),
    ] {
        if let Ok(value) = PrimitiveDateTime::parse(candidate, format) {
            return Some(value.assume_offset(offset));
        }
    }
    None
}

fn parse_explicit_date(candidate: &str) -> Option<Date> {
    for format in [
        format_description!("[year]-[month]-[day]"),
        format_description!("[day].[month].[year]"),
        format_description!("[year]/[month]/[day]"),
    ] {
        if let Ok(value) = Date::parse(candidate, format) {
            return Some(value);
        }
    }
    None
}

fn parse_relative_datetime(question: &str, now: OffsetDateTime) -> Option<OffsetDateTime> {
    let time = extract_time_of_day(question).unwrap_or(Time::from_hms(12, 0, 0).ok()?);
    let date = if question.contains("позавчера") {
        now.date() - Duration::days(2)
    } else if question.contains("вчера") {
        now.date() - Duration::days(1)
    } else if question.contains("сегодня") || question.contains("today") {
        now.date()
    } else if let Some((weekday, modifier)) = extract_weekday_reference(question) {
        match modifier {
            WeekdayModifier::Previous(weeks) => previous_weekday(now.date(), weekday, weeks),
            WeekdayModifier::Next(weeks) => next_weekday(now.date(), weekday, weeks),
            WeekdayModifier::Current => current_weekday(now.date(), weekday),
        }
    } else {
        return None;
    };

    Some(date.with_time(time).assume_offset(now.offset()))
}

fn extract_weekday_reference(question: &str) -> Option<(Weekday, WeekdayModifier)> {
    let weekday = detect_weekday(question)?;
    let modifier = if question.contains("позапрош") {
        WeekdayModifier::Previous(2)
    } else if question.contains("прошл")
        || question.contains("предыдущ")
        || question.contains("last ")
    {
        WeekdayModifier::Previous(1)
    } else if question.contains("следующ") || question.contains("next ") {
        WeekdayModifier::Next(1)
    } else {
        WeekdayModifier::Current
    };
    Some((weekday, modifier))
}

fn detect_weekday(question: &str) -> Option<Weekday> {
    let weekday_roots = [
        (Weekday::Monday, ["понедель", "monday", "mon"].as_slice()),
        (Weekday::Tuesday, ["вторник", "tuesday", "tue"].as_slice()),
        (Weekday::Wednesday, ["сред", "wednesday", "wed"].as_slice()),
        (Weekday::Thursday, ["четверг", "thursday", "thu"].as_slice()),
        (Weekday::Friday, ["пятниц", "friday", "fri"].as_slice()),
        (Weekday::Saturday, ["суббот", "saturday", "sat"].as_slice()),
        (Weekday::Sunday, ["воскрес", "sunday", "sun"].as_slice()),
    ];
    for (weekday, roots) in weekday_roots {
        if roots.iter().any(|root| question.contains(root)) {
            return Some(weekday);
        }
    }
    None
}

fn previous_weekday(mut date: Date, target: Weekday, count: u8) -> Date {
    let mut remaining = count.max(1);
    loop {
        date -= Duration::days(1);
        if date.weekday() == target {
            remaining -= 1;
            if remaining == 0 {
                return date;
            }
        }
    }
}

fn next_weekday(mut date: Date, target: Weekday, count: u8) -> Date {
    let mut remaining = count.max(1);
    loop {
        date += Duration::days(1);
        if date.weekday() == target {
            remaining -= 1;
            if remaining == 0 {
                return date;
            }
        }
    }
}

fn current_weekday(reference: Date, target: Weekday) -> Date {
    let start_of_week =
        reference - Duration::days(reference.weekday().number_days_from_monday().into());
    start_of_week + Duration::days(i64::from(target.number_days_from_monday()))
}

fn extract_time_of_day(question: &str) -> Option<Time> {
    for token in question.split_whitespace() {
        let candidate = trim_datetime_token(token);
        if candidate.is_empty() {
            continue;
        }
        if let Some(value) = parse_time_token(candidate) {
            return Some(value);
        }
    }
    None
}

fn parse_time_token(candidate: &str) -> Option<Time> {
    let mut parts = candidate.split(':');
    let hour = parts.next()?.parse::<u8>().ok()?;
    let minute = parts.next()?.parse::<u8>().ok()?;
    let second = parts
        .next()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    if parts.next().is_some() {
        return None;
    }
    Time::from_hms(hour, minute, second).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeekdayModifier {
    Previous(u8),
    Next(u8),
    Current,
}

fn extract_chats_ago_offset(question: &str) -> Option<usize> {
    if !question.contains("назад") {
        return None;
    }
    let tokens = tokenize_words(question);
    for (index, token) in tokens.iter().enumerate() {
        if !token.starts_with("чат") && token != "chat" {
            continue;
        }
        for candidate in tokens[..index].iter().rev().take(3) {
            if let Ok(value) = candidate.parse::<usize>() {
                return Some(value);
            }
            if let Some(value) = parse_small_number_word(candidate) {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{current_weekday, interpret};
    use time::{Date, Month, OffsetDateTime, Time, UtcOffset, Weekday};

    fn fixed_now() -> OffsetDateTime {
        Date::from_calendar_date(2026, Month::March, 21)
            .expect("date")
            .with_time(Time::from_hms(15, 0, 0).expect("time"))
            .assume_offset(UtcOffset::from_hms(3, 0, 0).expect("offset"))
    }

    #[test]
    fn interprets_previous_chat_with_message_count() {
        let parsed = interpret(
            "на чем закончили в прошлом чате, какие последние два сообщения?",
            fixed_now(),
        )
        .expect("parsed");

        assert_eq!(parsed.intent, "previous_chat");
        assert_eq!(parsed.chat_reference.as_deref(), Some("previous"));
        assert_eq!(parsed.messages_count, 2);
        assert!(parsed.include_chat_messages);
    }

    #[test]
    fn interprets_penultimate_chat_reference() {
        let parsed = interpret("что было в позапрошлом чате?", fixed_now()).expect("parsed");

        assert_eq!(parsed.intent, "previous_chat");
        assert_eq!(parsed.chat_reference.as_deref(), Some("previous:2"));
        assert!(parsed.include_chat_messages);
    }

    #[test]
    fn interprets_numeric_chats_ago_reference() {
        let parsed = interpret("что было 3 чата назад?", fixed_now()).expect("parsed");

        assert_eq!(parsed.intent, "previous_chat");
        assert_eq!(parsed.chat_reference.as_deref(), Some("previous:3"));
        assert!(parsed.include_chat_messages);
    }

    #[test]
    fn interprets_relative_russian_weekday_and_time() {
        let parsed =
            interpret("о чем мы говорили в прошлую среду в 12:00?", fixed_now()).expect("parsed");

        assert_eq!(parsed.intent, "chat_at_time");
        assert_eq!(
            parsed.at_time_rfc3339.as_deref(),
            Some("2026-03-18T12:00:00+03:00")
        );
        assert!(parsed.include_chat_messages);
    }

    #[test]
    fn interprets_penultimate_weekday_reference() {
        let parsed = interpret(
            "о чем мы говорили в позапрошлую среду в 12:00?",
            fixed_now(),
        )
        .expect("parsed");

        assert_eq!(parsed.intent, "chat_at_time");
        assert_eq!(
            parsed.at_time_rfc3339.as_deref(),
            Some("2026-03-11T12:00:00+03:00")
        );
    }

    #[test]
    fn interprets_explicit_datetime_without_offset_as_local() {
        let parsed = interpret("что было в чате 2026-03-20 11:41?", fixed_now()).expect("parsed");

        assert_eq!(parsed.intent, "chat_at_time");
        assert_eq!(
            parsed.at_time_rfc3339.as_deref(),
            Some("2026-03-20T11:41:00+03:00")
        );
    }

    #[test]
    fn interprets_generic_continuity_question_without_messages() {
        let parsed = interpret("на чем остановились в этом чате?", fixed_now()).expect("parsed");

        assert_eq!(parsed.intent, "last_chat");
        assert_eq!(parsed.chat_reference.as_deref(), Some("current"));
        assert!(parsed.include_chat_messages);
    }

    #[test]
    fn interprets_current_chat_even_with_last_message_phrase() {
        let parsed = interpret(
            "на чем остановились в текущем чате, покажи последнее сообщение",
            fixed_now(),
        )
        .expect("parsed");

        assert_eq!(parsed.intent, "last_chat");
        assert_eq!(parsed.chat_reference.as_deref(), Some("current"));
        assert_eq!(parsed.messages_count, 1);
        assert!(parsed.include_chat_messages);
    }

    #[test]
    fn interprets_last_chat_phrase_as_previous_chat() {
        let parsed = interpret("что было в последнем чате?", fixed_now()).expect("parsed");

        assert_eq!(parsed.intent, "previous_chat");
        assert_eq!(parsed.chat_reference.as_deref(), Some("previous"));
    }

    #[test]
    fn current_weekday_stays_inside_same_week() {
        let now = fixed_now();
        let resolved = current_weekday(now.date(), Weekday::Monday);
        assert_eq!(resolved.to_string(), "2026-03-16");
    }
}
