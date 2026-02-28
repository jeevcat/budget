package com.budget.shared.config

import android.content.Context

/** Android [ConfigStore] backed by SharedPreferences. */
class AndroidConfigStore(context: Context) : ConfigStore {

    private val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    override fun load(): ServerConfig? {
        val url = prefs.getString(KEY_SERVER_URL, null) ?: return null
        val key = prefs.getString(KEY_API_KEY, null) ?: return null
        return ServerConfig(serverUrl = url, apiKey = key)
    }

    override fun save(config: ServerConfig) {
        prefs.edit()
            .putString(KEY_SERVER_URL, config.serverUrl)
            .putString(KEY_API_KEY, config.apiKey)
            .apply()
    }

    override fun clear() {
        prefs.edit().clear().apply()
    }

    private companion object {
        const val PREFS_NAME = "budget_config"
        const val KEY_SERVER_URL = "server_url"
        const val KEY_API_KEY = "api_key"
    }
}
