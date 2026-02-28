package com.budget.shared.api

/** Abstraction over [BudgetApi.testConnection] so ViewModels are unit-testable. */
fun interface ConnectionTester {
    suspend fun testConnection(serverUrl: String, apiKey: String): ConnectionResult
}

/** Default implementation that delegates to a real [BudgetApi]. */
class DefaultConnectionTester : ConnectionTester {
    override suspend fun testConnection(serverUrl: String, apiKey: String): ConnectionResult {
        val api = BudgetApi(serverUrl, apiKey)
        return try {
            api.testConnection()
        } finally {
            api.close()
        }
    }
}
