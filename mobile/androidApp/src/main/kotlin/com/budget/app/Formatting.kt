package com.budget.app

import java.text.NumberFormat
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException
import java.util.Currency
import java.util.Locale
import kotlin.math.abs

val EuroCurrencyFormat: NumberFormat =
    NumberFormat.getCurrencyInstance(Locale.GERMANY).apply {
      currency = Currency.getInstance("EUR")
    }

val ShortDateFormatter: DateTimeFormatter = DateTimeFormatter.ofPattern("MMM d", Locale.ENGLISH)

val LongDateFormatter: DateTimeFormatter =
    DateTimeFormatter.ofPattern("MMMM d, yyyy", Locale.ENGLISH)

/** Format a numeric amount as whole euros with optional sign prefix. */
fun formatAmount(value: Double, showSign: Boolean = false): String {
  val formatted =
      synchronized(EuroCurrencyFormat) {
        EuroCurrencyFormat.maximumFractionDigits = 0
        EuroCurrencyFormat.format(abs(value))
      }
  val prefix =
      when {
        showSign && value > 0 -> "+"
        value < 0 -> "-"
        else -> ""
      }
  return "$prefix$formatted"
}

/** Format a string amount as whole euros with sign prefix (+/-). */
fun formatAmountSigned(value: String): String {
  val d = value.toDoubleOrNull() ?: return value
  return formatAmount(d, showSign = true)
}

/** Format a string amount as whole euros (negative gets "-", positive gets no sign). */
fun formatTransactionAmount(value: String): String {
  val d = value.toDoubleOrNull() ?: return value
  return formatAmount(d)
}

/** Format a nullable budget amount string, returning "No budget" for null. */
fun formatBudgetAmount(value: String?): String {
  if (value == null) return "No budget"
  val d = value.toDoubleOrNull() ?: return value
  return synchronized(EuroCurrencyFormat) {
    EuroCurrencyFormat.maximumFractionDigits = 0
    EuroCurrencyFormat.format(abs(d))
  }
}

/** Parse an ISO date string to LocalDate, returning null on failure. */
fun parseLocalDate(date: String): LocalDate? =
    try {
      LocalDate.parse(date)
    } catch (_: DateTimeParseException) {
      null
    }

/** Format an ISO date string as short date (e.g. "Mar 3"). */
fun formatShortDate(dateStr: String): String =
    try {
      LocalDate.parse(dateStr).format(ShortDateFormatter)
    } catch (_: DateTimeParseException) {
      dateStr
    }

/** Format an ISO date string as long date (e.g. "March 3, 2026"). */
fun formatLongDate(dateStr: String): String =
    try {
      LocalDate.parse(dateStr).format(LongDateFormatter)
    } catch (_: DateTimeParseException) {
      dateStr
    }
