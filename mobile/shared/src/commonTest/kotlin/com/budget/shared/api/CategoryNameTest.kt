package com.budget.shared.api

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFails
import kotlin.test.assertTrue
import kotlinx.serialization.json.Json

class CategoryNameTest {

  @Test
  fun validSimpleName() {
    val name = CategoryName.of("Groceries").getOrThrow()
    assertEquals("Groceries", name.value)
  }

  @Test
  fun validNameWithSpaces() {
    val name = CategoryName.of("Dining Out").getOrThrow()
    assertEquals("Dining Out", name.value)
  }

  @Test
  fun validNameWithUnicode() {
    val name = CategoryName.of("Essen & Trinken").getOrThrow()
    assertEquals("Essen & Trinken", name.value)
  }

  @Test
  fun validNameWithEmoji() {
    val name = CategoryName.of("Food \uD83C\uDF54").getOrThrow()
    assertEquals("Food \uD83C\uDF54", name.value)
  }

  @Test
  fun rejectsEmptyName() {
    val result = CategoryName.of("")
    assertTrue(result.isFailure)
    assertEquals("name is empty", result.exceptionOrNull()?.message)
  }

  @Test
  fun rejectsLeadingWhitespace() {
    val result = CategoryName.of(" Groceries")
    assertTrue(result.isFailure)
    assertEquals("name has leading or trailing whitespace", result.exceptionOrNull()?.message)
  }

  @Test
  fun rejectsTrailingWhitespace() {
    val result = CategoryName.of("Groceries ")
    assertTrue(result.isFailure)
    assertEquals("name has leading or trailing whitespace", result.exceptionOrNull()?.message)
  }

  @Test
  fun rejectsColon() {
    val result = CategoryName.of("Food:Groceries")
    assertTrue(result.isFailure)
    assertTrue(result.exceptionOrNull()?.message?.contains("colon") == true)
  }

  @Test
  fun rejectsControlCharacters() {
    val result = CategoryName.of("Bad\u0000Name")
    assertTrue(result.isFailure)
    assertTrue(result.exceptionOrNull()?.message?.contains("control") == true)
  }

  @Test
  fun rejectsNewline() {
    val result = CategoryName.of("Line\nBreak")
    assertTrue(result.isFailure)
    assertTrue(result.exceptionOrNull()?.message?.contains("control") == true)
  }

  @Test
  fun rejectsNameExceeding100Bytes() {
    val long = "a".repeat(101)
    val result = CategoryName.of(long)
    assertTrue(result.isFailure)
    assertTrue(result.exceptionOrNull()?.message?.contains("100 bytes") == true)
  }

  @Test
  fun acceptsNameAtExactly100Bytes() {
    val exact = "a".repeat(100)
    val name = CategoryName.of(exact).getOrThrow()
    assertEquals(exact, name.value)
  }

  @Test
  fun toStringReturnsValue() {
    val name = CategoryName.of("Transport").getOrThrow()
    assertEquals("Transport", name.toString())
  }

  @Test
  fun serializesToPlainJsonString() {
    val name = CategoryName.of("Rent").getOrThrow()
    val json = Json.encodeToString(CategoryName.serializer(), name)
    assertEquals("\"Rent\"", json)
  }

  @Test
  fun deserializesFromPlainJsonString() {
    val name = Json.decodeFromString(CategoryName.serializer(), "\"Insurance\"")
    assertEquals("Insurance", name.value)
  }

  @Test
  fun deserializationRejectsInvalidName() {
    assertFails { Json.decodeFromString(CategoryName.serializer(), "\"Food:Groceries\"") }
  }

  @Test
  fun multibyteBoundary() {
    // 34 × 3-byte chars = 102 bytes > 100 limit
    val tooLong = "\u00E9".repeat(34) // é = 2 bytes in UTF-8
    // 50 × 2-byte chars = 100 bytes, exactly at limit
    val atLimit = "\u00E9".repeat(50)
    assertTrue(CategoryName.of(tooLong).isFailure)
    assertTrue(CategoryName.of(atLimit).isSuccess)
  }
}
