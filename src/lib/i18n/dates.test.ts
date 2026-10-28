import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { calendarDays, durationLabel, fromNow, relativeAt, setLocale } from "./index";

// Local dates below are in Zurich time, which leaves summer time on 25 October 2026.
const zone = process.env.TZ;
beforeAll(() => {
  process.env.TZ = "Europe/Zurich";
});
afterAll(() => {
  process.env.TZ = zone;
});
afterEach(() => setLocale("en"));

/** 29 October 2026 at 22:30 is `at(29, 22, 30)`, local time. */
const at = (day: number, hour: number, minute = 0) => new Date(2026, 9, day, hour, minute);
// ICU puts a narrow no-break space before "PM".
const plain = (text: string) => text.replace(/\s/g, " ");

describe("calendarDays", () => {
  it("counts dates, not elapsed time", () => {
    expect(calendarDays(at(29, 22, 30), at(28, 8))).toBe(1);
    expect(calendarDays(at(29, 0, 10), at(28, 23, 50))).toBe(1);
    expect(calendarDays(at(30, 23), at(28, 0, 30))).toBe(2);
    expect(calendarDays(at(26, 22, 30), at(28, 8))).toBe(-2);
    expect(calendarDays(at(28, 23, 59), at(28, 0))).toBe(0);
  });

  it("isn't thrown off by a 25-hour day or a new year", () => {
    expect(calendarDays(at(26, 0), at(24, 23, 30))).toBe(2);
    expect(calendarDays(at(31, 23), at(24, 23, 30))).toBe(7);
    expect(calendarDays(new Date(2027, 0, 1, 0, 5), new Date(2026, 11, 31, 23, 55))).toBe(1);
  });
});

describe("fromNow", () => {
  it("says tomorrow for the next date, however many hours away", () => {
    // 38.5 hours, which rounded to "in 2 days" before.
    expect(fromNow(at(29, 22, 30), at(28, 8))).toBe("tomorrow");
    expect(fromNow(at(30, 1), at(27, 23))).toBe("in 3 days");
    expect(fromNow(at(26, 22, 30), at(28, 8))).toBe("2 days ago");
  });

  it("keeps hours and minutes under a day", () => {
    expect(fromNow(at(29, 22), at(28, 23))).toBe("in 23 hours");
    expect(fromNow(at(28, 8, 40), at(28, 8))).toBe("in 40 minutes");
    expect(fromNow(at(28, 7, 50), at(28, 8))).toBe("10 minutes ago");
    expect(fromNow(at(28, 8, 0), at(28, 8))).toBe("now");
  });

  it("uses hours for a whole day on one date, when the clocks go back", () => {
    // 25 October has 25 hours: midnight to 23:30 is 24.5 hours.
    expect(fromNow(at(25, 23, 30), at(25, 0))).toBe("in 25 hours");
  });

  it("follows the UI's language", () => {
    setLocale("fr");
    expect(fromNow(at(29, 22, 30), at(28, 8))).toBe("demain");
    expect(fromNow(at(30, 9), at(28, 8))).toBe("après-demain");
    setLocale("de");
    expect(fromNow(at(26, 22, 30), at(28, 8))).toBe("vorgestern");
  });
});

describe("relativeAt", () => {
  it("names the day and the time up to two dates away", () => {
    expect(plain(relativeAt(at(29, 22, 30), at(28, 8)))).toBe("tomorrow at 10:30 PM");
    expect(plain(relativeAt(at(28, 22, 30), at(28, 8)))).toBe("today at 10:30 PM");
    expect(plain(relativeAt(at(29, 7), at(28, 8)))).toBe("tomorrow at 7:00 AM");
    expect(plain(relativeAt(at(30, 23), at(28, 0, 30)))).toBe("in 2 days at 11:00 PM");
  });

  it("mirrors it for a deadline that passed", () => {
    expect(plain(relativeAt(at(27, 22, 30), at(28, 8)))).toBe("yesterday at 10:30 PM");
    expect(plain(relativeAt(at(26, 22, 30), at(28, 8)))).toBe("2 days ago at 10:30 PM");
  });

  it("counts minutes within the hour and days further out", () => {
    expect(relativeAt(at(28, 8, 40), at(28, 8))).toBe("in 40 minutes");
    expect(relativeAt(at(29, 0, 10), at(28, 23, 50))).toBe("in 20 minutes");
    expect(relativeAt(at(28, 7, 50), at(28, 8))).toBe("10 minutes ago");
    expect(relativeAt(at(31, 22, 30), at(28, 8))).toBe("in 3 days");
    expect(relativeAt(at(25, 7), at(28, 8))).toBe("3 days ago");
  });

  it("names the hour instead of counting down when asked to", () => {
    expect(plain(relativeAt(at(28, 8, 40), at(28, 8), false))).toBe("today at 8:40 AM");
    expect(plain(relativeAt(at(28, 7, 50), at(28, 8), false))).toBe("today at 7:50 AM");
    expect(plain(relativeAt(at(29, 0, 10), at(28, 23, 50), false))).toBe("tomorrow at 12:10 AM");
    // Further out the words already change at midnight only.
    expect(relativeAt(at(31, 22, 30), at(28, 8), false)).toBe("in 3 days");
  });

  it("keeps the dates straight across the change of time", () => {
    // 48.5 hours across the extra hour of 25 October, two dates away.
    expect(plain(relativeAt(at(26, 11, 30), at(24, 12)))).toBe("in 2 days at 11:30 AM");
    expect(plain(relativeAt(at(25, 23, 30), at(25, 0)))).toBe("today at 11:30 PM");
    expect(relativeAt(at(31, 23), at(24, 23, 30))).toBe("in 7 days");
  });

  it("speaks the UI's language", () => {
    setLocale("fr");
    expect(relativeAt(at(29, 22, 30), at(28, 8))).toBe("demain à 22:30");
    expect(relativeAt(at(30, 9, 5), at(28, 8))).toBe("après-demain à 9:05");
    expect(relativeAt(at(27, 22, 30), at(28, 8))).toBe("hier à 22:30");
    setLocale("de");
    expect(relativeAt(at(29, 22, 30), at(28, 8))).toBe("morgen um 22:30");
    expect(relativeAt(at(28, 13), at(28, 8))).toBe("heute um 13:00");
  });
});

describe("durationLabel", () => {
  it("still counts elapsed time, for lateness", () => {
    expect(durationLabel(30 * 3600)).toBe("1 day");
    expect(durationLabel(47 * 3600)).toBe("1 day");
  });
});
