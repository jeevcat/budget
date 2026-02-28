package com.budget.shared.viewmodel

import com.budget.shared.api.ConnectionResult
import com.budget.shared.api.ConnectionTester
import com.budget.shared.config.ConfigStore
import com.budget.shared.config.ServerConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

@OptIn(ExperimentalCoroutinesApi::class)
class SetupViewModelTest {

    private val testDispatcher = UnconfinedTestDispatcher()

    @BeforeTest
    fun setUp() {
        Dispatchers.setMain(testDispatcher)
    }

    @AfterTest
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun initialStateIsEmpty() {
        val vm = SetupViewModel(FakeConfigStore(), FakeConnectionTester())
        assertEquals(SetupUiState(), vm.uiState.value)
    }

    @Test
    fun updateServerUrlClearsError() {
        val vm = SetupViewModel(FakeConfigStore(), FakeConnectionTester())
        // Force an error first
        vm.connect()
        assertEquals("Both fields are required", vm.uiState.value.errorMessage)

        vm.updateServerUrl("https://example.com")
        assertEquals("https://example.com", vm.uiState.value.serverUrl)
        assertNull(vm.uiState.value.errorMessage)
    }

    @Test
    fun updateApiKeyClearsError() {
        val vm = SetupViewModel(FakeConfigStore(), FakeConnectionTester())
        vm.connect()
        assertEquals("Both fields are required", vm.uiState.value.errorMessage)

        vm.updateApiKey("secret")
        assertEquals("secret", vm.uiState.value.apiKey)
        assertNull(vm.uiState.value.errorMessage)
    }

    @Test
    fun connectWithEmptyFieldsShowsError() {
        val vm = SetupViewModel(FakeConfigStore(), FakeConnectionTester())
        vm.connect()
        assertEquals("Both fields are required", vm.uiState.value.errorMessage)
        assertEquals(false, vm.uiState.value.loading)
    }

    @Test
    fun connectWithEmptyUrlShowsError() {
        val vm = SetupViewModel(FakeConfigStore(), FakeConnectionTester())
        vm.updateApiKey("key")
        vm.connect()
        assertEquals("Both fields are required", vm.uiState.value.errorMessage)
    }

    @Test
    fun connectWithEmptyKeyShowsError() {
        val vm = SetupViewModel(FakeConfigStore(), FakeConnectionTester())
        vm.updateServerUrl("https://example.com")
        vm.connect()
        assertEquals("Both fields are required", vm.uiState.value.errorMessage)
    }

    @Test
    fun connectSuccessSavesConfigAndSetsConnected() = runTest {
        val store = FakeConfigStore()
        val tester = FakeConnectionTester(ConnectionResult.Success)
        val vm = SetupViewModel(store, tester)

        vm.updateServerUrl("https://example.com")
        vm.updateApiKey("key123")
        vm.connect()

        val state = vm.uiState.value
        assertEquals(false, state.loading)
        assertNull(state.errorMessage)
        assertEquals(ServerConfig("https://example.com", "key123"), state.connectedConfig)
        assertEquals(ServerConfig("https://example.com", "key123"), store.saved)
    }

    @Test
    fun connectTrimsInputs() = runTest {
        val store = FakeConfigStore()
        val tester = FakeConnectionTester(ConnectionResult.Success)
        val vm = SetupViewModel(store, tester)

        vm.updateServerUrl("  https://example.com/  ")
        vm.updateApiKey("  key123  ")
        vm.connect()

        assertEquals(ServerConfig("https://example.com", "key123"), vm.uiState.value.connectedConfig)
    }

    @Test
    fun connectServerUnreachableShowsError() = runTest {
        val msg = "Could not reach server"
        val vm = SetupViewModel(
            FakeConfigStore(),
            FakeConnectionTester(ConnectionResult.ServerUnreachable(msg)),
        )

        vm.updateServerUrl("https://example.com")
        vm.updateApiKey("key")
        vm.connect()

        assertEquals(msg, vm.uiState.value.errorMessage)
        assertEquals(false, vm.uiState.value.loading)
        assertNull(vm.uiState.value.connectedConfig)
    }

    @Test
    fun connectAuthFailedShowsError() = runTest {
        val msg = "API key was rejected"
        val vm = SetupViewModel(
            FakeConfigStore(),
            FakeConnectionTester(ConnectionResult.AuthFailed(msg)),
        )

        vm.updateServerUrl("https://example.com")
        vm.updateApiKey("bad-key")
        vm.connect()

        assertEquals(msg, vm.uiState.value.errorMessage)
        assertNull(vm.uiState.value.connectedConfig)
    }

    @Test
    fun connectGenericErrorShowsError() = runTest {
        val msg = "Something went wrong"
        val vm = SetupViewModel(
            FakeConfigStore(),
            FakeConnectionTester(ConnectionResult.Error(msg)),
        )

        vm.updateServerUrl("https://example.com")
        vm.updateApiKey("key")
        vm.connect()

        assertEquals(msg, vm.uiState.value.errorMessage)
        assertNull(vm.uiState.value.connectedConfig)
    }
}

// -- Test doubles -------------------------------------------------------

private class FakeConfigStore : ConfigStore {
    var saved: ServerConfig? = null
        private set

    override fun load(): ServerConfig? = null
    override fun save(config: ServerConfig) { saved = config }
    override fun clear() { saved = null }
}

private class FakeConnectionTester(
    private val result: ConnectionResult = ConnectionResult.Success,
) : ConnectionTester {
    override suspend fun testConnection(serverUrl: String, apiKey: String): ConnectionResult = result
}
