package com.budget.shared.api

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.client.request.delete
import io.ktor.client.request.get
import io.ktor.client.request.header
import io.ktor.client.request.post
import io.ktor.client.request.put
import io.ktor.client.request.setBody
import io.ktor.http.ContentType
import io.ktor.http.contentType
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
    install(ContentNegotiation) { json(Json { ignoreUnknownKeys = true }) }
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
      val response =
          client.get(url("/api/jobs/counts")) { header("Authorization", "Bearer $apiKey") }
      response.status.isSuccess()
    } catch (_: Exception) {
      false
    }
  }

  /**
   * Full connection check: first verifies server is reachable, then validates the API key against
   * an authenticated endpoint.
   */
  suspend fun testConnection(): ConnectionResult {
    return try {
      if (!checkHealth()) {
        return ConnectionResult.ServerUnreachable("Could not reach server at $baseUrl")
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
    val path =
        if (monthId != null) "/api/budgets/status?month_id=$monthId" else "/api/budgets/status"
    val response = client.get(url(path)) { header("Authorization", "Bearer $apiKey") }
    return response.body()
  }

  /** Fetch all available budget months. */
  suspend fun getMonths(): List<BudgetMonth> {
    val response =
        client.get(url("/api/budgets/months")) { header("Authorization", "Bearer $apiKey") }
    return response.body()
  }

  /** Fetch paginated transactions with optional filters. */
  suspend fun getTransactions(
      limit: Int = 50,
      offset: Int = 0,
      categoryId: String? = null,
      search: String? = null,
      categoryMethod: String? = null,
  ): TransactionPage {
    val params = buildList {
      add("limit=$limit")
      add("offset=$offset")
      if (categoryId != null) add("category_id=$categoryId")
      if (search != null) add("search=$search")
      if (categoryMethod != null) add("category_method=$categoryMethod")
    }
    val query = params.joinToString("&")
    val response =
        client.get(url("/api/transactions?$query")) { header("Authorization", "Bearer $apiKey") }
    return response.body()
  }

  /** Fetch a single transaction by ID. */
  suspend fun getTransaction(id: String): Transaction {
    val response =
        client.get(url("/api/transactions/$id")) { header("Authorization", "Bearer $apiKey") }
    return response.body()
  }

  /** Fetch all categories. */
  suspend fun getCategories(): List<Category> {
    val response = client.get(url("/api/categories")) { header("Authorization", "Bearer $apiKey") }
    return response.body()
  }

  /** Create a new category. Returns the created category. */
  suspend fun createCategory(request: CategoryRequest): Category {
    val response =
        client.post(url("/api/categories")) {
          header("Authorization", "Bearer $apiKey")
          contentType(ContentType.Application.Json)
          setBody(request)
        }
    return response.body()
  }

  /** Update an existing category. Returns the updated category. */
  suspend fun updateCategory(id: String, request: CategoryRequest): Category {
    val response =
        client.put(url("/api/categories/$id")) {
          header("Authorization", "Bearer $apiKey")
          contentType(ContentType.Application.Json)
          setBody(request)
        }
    return response.body()
  }

  /** Assign a category to a transaction. Returns true on success. */
  suspend fun categorizeTransaction(transactionId: String, categoryId: String): Boolean {
    return try {
      val response =
          client.post(url("/api/transactions/$transactionId/categorize")) {
            header("Authorization", "Bearer $apiKey")
            contentType(ContentType.Application.Json)
            setBody(CategorizeRequest(categoryId))
          }
      response.status.isSuccess()
    } catch (_: Exception) {
      false
    }
  }

  /** Clear a transaction's category. Returns true on success. */
  suspend fun uncategorizeTransaction(transactionId: String): Boolean {
    return try {
      val response =
          client.delete(url("/api/transactions/$transactionId/categorize")) {
            header("Authorization", "Bearer $apiKey")
          }
      response.status.isSuccess()
    } catch (_: Exception) {
      false
    }
  }

  fun close() {
    client.close()
  }
}
