package com.budget.shared.api

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.client.request.get
import io.ktor.client.request.header
import io.ktor.http.isSuccess
import io.ktor.serialization.kotlinx.json.json
import kotlinx.serialization.json.Json

sealed class ConnectionResult {
    data object Success : ConnectionResult()
    data class ServerUnreachable(val message: String) : ConnectionResult()
    data class AuthFailed(val message: String) : ConnectionResult()
    data class Error(val message: String) : ConnectionResult()
}

class BudgetApi(private val baseUrl: String, private val apiKey: String) {

    private val client = HttpClient {
        install(ContentNegotiation) {
            json(Json { ignoreUnknownKeys = true })
        }
    }

    private fun url(path: String): String {
        val base = baseUrl.trimEnd('/')
        return "$base$path"
    }

    /** Check server reachability via the unauthenticated /health endpoint. */
    suspend fun checkHealth(): Boolean {
        return try {
            val response = client.get(url("/health"))
            response.status.isSuccess()
        } catch (_: Exception) {
            false
        }
    }

    /** Verify the API key by hitting an authenticated endpoint. */
    suspend fun verifyAuth(): Boolean {
        return try {
            val response = client.get(url("/api/jobs/counts")) {
                header("Authorization", "Bearer $apiKey")
            }
            response.status.isSuccess()
        } catch (_: Exception) {
            false
        }
    }

    /**
     * Full connection check: first verifies server is reachable,
     * then validates the API key against an authenticated endpoint.
     */
    suspend fun testConnection(): ConnectionResult {
        return try {
            if (!checkHealth()) {
                return ConnectionResult.ServerUnreachable(
                    "Could not reach server at $baseUrl"
                )
            }
            if (!verifyAuth()) {
                return ConnectionResult.AuthFailed("Server reachable but API key was rejected")
            }
            ConnectionResult.Success
        } catch (e: Exception) {
            ConnectionResult.Error(e.message ?: "Unknown error")
        }
    }

    /** Fetch budget status for the current or a specific month. */
    suspend fun getStatus(monthId: String? = null): StatusResponse {
        val path = if (monthId != null) "/api/budgets/status?month_id=$monthId" else "/api/budgets/status"
        val response = client.get(url(path)) {
            header("Authorization", "Bearer $apiKey")
        }
        return response.body()
    }

    /** Fetch all available budget months. */
    suspend fun getMonths(): List<BudgetMonth> {
        val response = client.get(url("/api/budgets/months")) {
            header("Authorization", "Bearer $apiKey")
        }
        return response.body()
    }

    fun close() {
        client.close()
    }
}
