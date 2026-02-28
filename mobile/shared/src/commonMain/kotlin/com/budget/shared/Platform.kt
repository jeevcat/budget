package com.budget.shared

interface Platform {
  val name: String
}

expect fun getPlatform(): Platform
