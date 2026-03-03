package com.budget.shared.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.budget.shared.api.Category
import com.budget.shared.api.CategoryMethod
import com.budget.shared.api.Transaction
import com.budget.shared.repository.BudgetRepository
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** A category with its resolved parent name for display. */
data class DisplayCategory(
    val id: String,
    val name: String,
    val parentName: String? = null,
    val depth: Int = 0,
) {
  val displayName: String
    get() = if (parentName != null) "$parentName > $name" else name
}

data class TransactionsUiState(
    val loading: Boolean = true,
    val error: String? = null,
    val transactions: List<Transaction> = emptyList(),
    val total: Int = 0,
    val categories: List<DisplayCategory> = emptyList(),
    /** The transaction currently open for category assignment. */
    val selectedTransaction: Transaction? = null,
    /** True while fetching a single transaction for the detail screen. */
    val detailLoading: Boolean = false,
    /** True while a categorize/uncategorize request is in flight. */
    val categorizing: Boolean = false,
    /** Search query for filtering the category picker. */
    val categorySearch: String = "",
)

class TransactionsViewModel(
    private val repository: BudgetRepository,
) : ViewModel() {

  private val _uiState = MutableStateFlow(TransactionsUiState())
  val uiState: StateFlow<TransactionsUiState> = _uiState.asStateFlow()

  init {
    load()
    viewModelScope.launch { repository.invalidationEvents.collect { load() } }
  }

  fun refresh() {
    load()
  }

  fun selectTransaction(transaction: Transaction?) {
    _uiState.update { it.copy(selectedTransaction = transaction, categorySearch = "") }
  }

  /** Fetch a transaction by ID and select it for detail view. */
  fun selectTransactionById(id: String) {
    _uiState.update { it.copy(detailLoading = true, selectedTransaction = null, error = null) }
    viewModelScope.launch {
      try {
        val txn = repository.getTransaction(id)
        _uiState.update {
          it.copy(detailLoading = false, selectedTransaction = txn, categorySearch = "")
        }
      } catch (e: Exception) {
        _uiState.update { it.copy(detailLoading = false, error = e.message ?: "Unknown error") }
      }
    }
  }

  fun updateCategorySearch(query: String) {
    _uiState.update { it.copy(categorySearch = query) }
  }

  /** Filtered categories based on the current search query. */
  fun filteredCategories(): List<DisplayCategory> {
    val state = _uiState.value
    val query = state.categorySearch.trim()
    if (query.isEmpty()) return state.categories
    return state.categories.filter { it.displayName.contains(query, ignoreCase = true) }
  }

  /** Assign a category to the currently selected transaction. */
  fun categorize(categoryId: String) {
    val txn = _uiState.value.selectedTransaction ?: return
    _uiState.update { it.copy(categorizing = true) }
    viewModelScope.launch {
      try {
        val success = repository.categorizeTransaction(txn.id, categoryId)
        if (success) {
          _uiState.update { state ->
            val categorized =
                txn.copy(categoryId = categoryId, categoryMethod = CategoryMethod.MANUAL)
            val updated = state.transactions.map { t -> if (t.id == txn.id) categorized else t }
            state.copy(
                categorizing = false,
                selectedTransaction = categorized,
                transactions = updated.filter { it.categoryId == null },
                total = (state.total - 1).coerceAtLeast(0),
            )
          }
        } else {
          _uiState.update {
            it.copy(
                categorizing = false,
                error = "Failed to categorize transaction",
            )
          }
        }
      } catch (e: Exception) {
        _uiState.update {
          it.copy(
              categorizing = false,
              error = e.message ?: "Unknown error",
          )
        }
      }
    }
  }

  /** Clear the category on the selected transaction (re-run auto-categorization). */
  fun uncategorize() {
    val txn = _uiState.value.selectedTransaction ?: return
    if (txn.categoryId == null) return
    _uiState.update { it.copy(categorizing = true) }
    viewModelScope.launch {
      try {
        val success = repository.uncategorizeTransaction(txn.id)
        if (success) {
          _uiState.update { state ->
            val cleared =
                txn.copy(categoryId = null, categoryMethod = null, llmJustification = null)
            val updated = state.transactions.map { t -> if (t.id == txn.id) cleared else t }
            state.copy(
                categorizing = false,
                selectedTransaction = cleared,
                transactions = updated,
            )
          }
        } else {
          _uiState.update {
            it.copy(
                categorizing = false,
                error = "Failed to uncategorize transaction",
            )
          }
        }
      } catch (e: Exception) {
        _uiState.update {
          it.copy(
              categorizing = false,
              error = e.message ?: "Unknown error",
          )
        }
      }
    }
  }

  /** Accept the AI-suggested category for the currently selected transaction. */
  fun acceptSuggestion() {
    val txn = _uiState.value.selectedTransaction ?: return
    val suggestion = txn.suggestedCategory ?: return
    val categories = _uiState.value.categories
    val match =
        categories.find { it.displayName.equals(suggestion, ignoreCase = true) }
            ?: categories.find { it.name.equals(suggestion, ignoreCase = true) }
            ?: return
    categorize(match.id)
  }

  fun clearError() {
    _uiState.update { it.copy(error = null) }
  }

  private fun load() {
    _uiState.update { it.copy(loading = true, error = null) }
    viewModelScope.launch {
      try {
        val page =
            repository.getTransactions(
                limit = 200,
                offset = 0,
                categoryId = "__none",
            )
        val rawCategories = repository.getCategories()
        val displayCategories = buildDisplayCategories(rawCategories)
        _uiState.update {
          it.copy(
              loading = false,
              transactions = page.items,
              total = page.total,
              categories = displayCategories,
          )
        }
      } catch (e: Exception) {
        _uiState.update { it.copy(loading = false, error = e.message ?: "Unknown error") }
      }
    }
  }

  companion object {
    /**
     * Build a sorted, hierarchical display list of leaf categories only.
     *
     * Parent categories (those that have children) are excluded from the list since transactions
     * should only be assigned to leaf categories. Children still display their parent name for
     * context. Root categories without children are included as selectable leaves.
     */
    fun buildDisplayCategories(raw: List<Category>): List<DisplayCategory> {
      val byId = raw.associateBy { it.id }
      val roots = raw.filter { it.parentId == null }.sortedBy { it.name.value.lowercase() }
      val childrenOf =
          raw.filter { it.parentId != null }
              .groupBy { it.parentId }
              .mapValues { (_, v) -> v.sortedBy { it.name.value.lowercase() } }

      return buildList {
        for (root in roots) {
          val children = childrenOf[root.id].orEmpty()
          if (children.isEmpty()) {
            add(DisplayCategory(id = root.id, name = root.name.value, depth = 0))
          } else {
            for (child in children) {
              add(
                  DisplayCategory(
                      id = child.id,
                      name = child.name.value,
                      parentName = root.name.value,
                      depth = 1,
                  )
              )
            }
          }
        }
      }
    }
  }
}
