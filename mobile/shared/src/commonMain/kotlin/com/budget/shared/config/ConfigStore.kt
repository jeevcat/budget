package com.budget.shared.config

/** Platform-agnostic interface for persisting server configuration. */
interface ConfigStore {
  fun load(): ServerConfig?

  fun save(config: ServerConfig)

  fun clear()
}
