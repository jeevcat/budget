package com.budget.shared.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.budget.shared.api.ConnectionResult
import com.budget.shared.api.ConnectionTester
import com.budget.shared.api.DefaultConnectionTester
import com.budget.shared.config.ConfigStore
import com.budget.shared.config.ServerConfig
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class SetupUiState(
    val serverUrl: String = "",
    val apiKey: String = "",
    val loading: Boolean = false,
    val errorMessage: String? = null,
    /** Non-null once a connection succeeds and the config has been persisted. */
    val connectedConfig: ServerConfig? = null,
)

class SetupViewModel(
    private val configStore: ConfigStore,
    private val connectionTester: ConnectionTester = DefaultConnectionTester(),
) : ViewModel() {

  private val _uiState = MutableStateFlow(SetupUiState())
  val uiState: StateFlow<SetupUiState> = _uiState.asStateFlow()

  fun updateServerUrl(url: String) {
    _uiState.update { it.copy(serverUrl = url, errorMessage = null) }
  }

  fun updateApiKey(key: String) {
    _uiState.update { it.copy(apiKey = key, errorMessage = null) }
  }

  fun connect() {
    val state = _uiState.value
    val trimmedUrl = state.serverUrl.trim().trimEnd('/')
    val trimmedKey = state.apiKey.trim()

    if (trimmedUrl.isEmpty() || trimmedKey.isEmpty()) {
      _uiState.update { it.copy(errorMessage = "Both fields are required") }
      return
    }

    _uiState.update { it.copy(loading = true, errorMessage = null) }

    viewModelScope.launch {
      val result = connectionTester.testConnection(trimmedUrl, trimmedKey)
      when (result) {
        is ConnectionResult.Success -> {
          val config = ServerConfig(trimmedUrl, trimmedKey)
          configStore.save(config)
          _uiState.update { it.copy(loading = false, connectedConfig = config) }
        }
        is ConnectionResult.ServerUnreachable -> {
          _uiState.update { it.copy(loading = false, errorMessage = result.message) }
        }
        is ConnectionResult.AuthFailed -> {
          _uiState.update { it.copy(loading = false, errorMessage = result.message) }
        }
        is ConnectionResult.Error -> {
          _uiState.update { it.copy(loading = false, errorMessage = result.message) }
        }
      }
    }
  }
}
