package com.budget.shared.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.budget.shared.api.BudgetApi
import com.budget.shared.api.BudgetMode
import com.budget.shared.api.Category
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** A category prepared for display with parent context and hierarchy info. */
data class CategoryDisplayItem(
    val id: String,
    val name: String,
    val parentName: String? = null,
    val budgetMode: BudgetMode? = null,
    val budgetAmount: String? = null,
    val transactionCount: Int = 0,
    val isChild: Boolean = false,
)

/** A section grouping categories by budget mode. */
data class CategorySection(
    val mode: BudgetMode?,
    val label: String,
    val categories: List<CategoryDisplayItem>,
)

data class CategoriesUiState(
    val loading: Boolean = true,
    val error: String? = null,
    val sections: List<CategorySection> = emptyList(),
    val expandedSections: Set<BudgetMode?> = emptySet(),
)

/** Abstraction over [BudgetApi] fetches so the ViewModel is unit-testable. */
fun interface CategoriesFetcher {
  suspend fun fetchCategories(serverUrl: String, apiKey: String): List<Category>
}

class DefaultCategoriesFetcher : CategoriesFetcher {
  override suspend fun fetchCategories(serverUrl: String, apiKey: String): List<Category> {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      api.getCategories()
    } finally {
      api.close()
    }
  }
}

class CategoriesViewModel(
    private val serverUrl: String,
    private val apiKey: String,
    private val fetcher: CategoriesFetcher = DefaultCategoriesFetcher(),
) : ViewModel() {

  private val _uiState = MutableStateFlow(CategoriesUiState())
  val uiState: StateFlow<CategoriesUiState> = _uiState.asStateFlow()

  init {
    refresh()
  }

  fun refresh() {
    _uiState.update { it.copy(loading = true, error = null) }
    viewModelScope.launch {
      try {
        val categories = fetcher.fetchCategories(serverUrl, apiKey)
        val sections = buildSections(categories)
        val allModes = sections.map { it.mode }.toSet()
        _uiState.update {
          it.copy(
              loading = false,
              sections = sections,
              expandedSections = allModes,
          )
        }
      } catch (e: Exception) {
        _uiState.update { it.copy(loading = false, error = e.message ?: "Unknown error") }
      }
    }
  }

  fun toggleSection(mode: BudgetMode?) {
    _uiState.update { state ->
      val expanded =
          if (mode in state.expandedSections) {
            state.expandedSections - mode
          } else {
            state.expandedSections + mode
          }
      state.copy(expandedSections = expanded)
    }
  }

  companion object {
    /** Group categories by budget mode with parent/child hierarchy. */
    fun buildSections(categories: List<Category>): List<CategorySection> {
      val byId = categories.associateBy { it.id }
      val childrenOf =
          categories
              .filter { it.parentId != null }
              .groupBy { it.parentId }
              .mapValues { (_, v) -> v.sortedBy { it.name.lowercase() } }

      val grouped = categories.groupBy { it.budgetMode }

      val sectionOrder: List<BudgetMode?> =
          listOf(BudgetMode.MONTHLY, BudgetMode.ANNUAL, BudgetMode.PROJECT, null)

      return sectionOrder.mapNotNull { mode ->
        val cats = grouped[mode] ?: return@mapNotNull null
        val roots = cats.filter { it.parentId == null }.sortedBy { it.name.lowercase() }

        val items = buildList {
          for (root in roots) {
            add(
                CategoryDisplayItem(
                    id = root.id,
                    name = root.name,
                    budgetMode = root.budgetMode,
                    budgetAmount = root.budgetAmount,
                    transactionCount = root.transactionCount,
                )
            )
            val children = childrenOf[root.id].orEmpty().filter { it.budgetMode == mode }
            for (child in children) {
              add(
                  CategoryDisplayItem(
                      id = child.id,
                      name = child.name,
                      parentName = root.name,
                      budgetMode = child.budgetMode,
                      budgetAmount = child.budgetAmount,
                      transactionCount = child.transactionCount,
                      isChild = true,
                  )
              )
            }
          }
        }

        if (items.isEmpty()) return@mapNotNull null

        CategorySection(
            mode = mode,
            label =
                when (mode) {
                  BudgetMode.MONTHLY -> "Monthly"
                  BudgetMode.ANNUAL -> "Annual"
                  BudgetMode.PROJECT -> "Project"
                  null -> "Unbudgeted"
                },
            categories = items,
        )
      }
    }
  }
}
