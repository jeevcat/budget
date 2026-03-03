package com.budget.shared.api

import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder

/** Maximum length for a category name (UTF-8 bytes), matching the backend. */
private const val MAX_CATEGORY_NAME_BYTES = 100

/**
 * A validated category name.
 *
 * Invariants enforced at construction (mirroring the Rust `CategoryName` newtype):
 * - Non-empty after trimming
 * - No leading or trailing whitespace
 * - No colons — hierarchy is expressed via `parent_id`, not embedded in names
 * - At most [MAX_CATEGORY_NAME_BYTES] UTF-8 bytes
 * - No control characters (U+0000–U+001F, U+007F–U+009F)
 */
@Serializable(with = CategoryNameSerializer::class)
class CategoryName private constructor(val value: String) {

  override fun equals(other: Any?): Boolean = other is CategoryName && value == other.value

  override fun hashCode(): Int = value.hashCode()

  companion object {
    /**
     * Create a [CategoryName], validating all invariants.
     *
     * @return a [Result] containing the validated name, or an error describing the violation.
     */
    fun of(name: String): Result<CategoryName> {
      if (name.isEmpty()) {
        return Result.failure(InvalidCategoryNameException("name is empty"))
      }
      if (name != name.trim()) {
        return Result.failure(
            InvalidCategoryNameException("name has leading or trailing whitespace")
        )
      }
      if (':' in name) {
        return Result.failure(
            InvalidCategoryNameException("name contains a colon \u2014 use parent_id for hierarchy")
        )
      }
      if (name.encodeToByteArray().size > MAX_CATEGORY_NAME_BYTES) {
        return Result.failure(
            InvalidCategoryNameException("name exceeds $MAX_CATEGORY_NAME_BYTES bytes")
        )
      }
      if (name.any { it.isISOControl() }) {
        return Result.failure(InvalidCategoryNameException("name contains control characters"))
      }
      return Result.success(CategoryName(name))
    }
  }

  override fun toString(): String = value
}

class InvalidCategoryNameException(message: String) : IllegalArgumentException(message)

/** Serializes [CategoryName] as a plain JSON string, validating on deserialization. */
internal object CategoryNameSerializer : KSerializer<CategoryName> {
  override val descriptor: SerialDescriptor =
      PrimitiveSerialDescriptor("CategoryName", PrimitiveKind.STRING)

  override fun serialize(encoder: Encoder, value: CategoryName) {
    encoder.encodeString(value.value)
  }

  override fun deserialize(decoder: Decoder): CategoryName {
    val raw = decoder.decodeString()
    return CategoryName.of(raw).getOrThrow()
  }
}
