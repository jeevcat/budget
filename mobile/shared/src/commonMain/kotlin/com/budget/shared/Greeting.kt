package com.budget.shared

class Greeting {
  private val platform = getPlatform()

  fun greet(): String = "Budget on ${platform.name}"
}
