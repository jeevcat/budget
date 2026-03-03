package com.budget.shared.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.budget.shared.api.BudgetMode
import com.budget.shared.api.Category
import com.budget.shared.repository.BudgetRepository
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
    val categories: List<Category> = emptyList(),
)

class CategoriesViewModel(
    private val repository: BudgetRepository,
) : ViewModel() {

  private val _uiState = MutableStateFlow(CategoriesUiState())
  val uiState: StateFlow<CategoriesUiState> = _uiState.asStateFlow()

  init {
    refresh()
    viewModelScope.launch { repository.invalidationEvents.collect { refresh() } }
  }

  fun refresh() {
    _uiState.update { it.copy(loading = true, error = null) }
    viewModelScope.launch {
      try {
        val categories = repository.getCategories()
        val sections = buildSections(categories)
        val allModes = sections.map { it.mode }.toSet()
        _uiState.update {
          it.copy(
              loading = false,
              sections = sections,
              expandedSections = allModes,
              categories = categories,
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
    /**
     * Extract the leaf (display) name from a category name, stripping the parent prefix if present.
     *
     * Handles both naming conventions:
     * - `"Groceries"` under parent `"Food"` → `"Groceries"`
     * - `"Food:Groceries"` under parent `"Food"` → `"Groceries"`
     */
    fun leafName(name: String, parentName: String?): String {
      if (parentName != null) {
        val prefix = "$parentName:"
        if (name.startsWith(prefix)) {
          return name.removePrefix(prefix)
        }
      }
      return name
    }

    /** Convenience overload accepting a [CategoryName]. */
    fun leafName(name: com.budget.shared.api.CategoryName, parentName: String?): String =
        leafName(name.value, parentName)

    /**
     * Group categories by budget mode with parent/child hierarchy.
     *
     * Each category's own [Category.budgetMode] determines its section. Children that have their
     * own budget_mode appear as top-level entries in that section. Children without a budget_mode
     * are nested under their parent in whichever section the parent belongs to.
     */
    fun buildSections(categories: List<Category>): List<CategorySection> {
      val byId = categories.associateBy { it.id }

      // Children without their own budget_mode, keyed by parent ID
      val unbudgetedChildrenOf =
          categories
              .filter { it.parentId != null && it.budgetMode == null }
              .groupBy { it.parentId }
              .mapValues { (_, v) -> v.sortedBy { it.name.value.lowercase() } }

      // Every category that has a budget_mode set, grouped by that mode.
      // Categories without budget_mode and without a parent go into the null group.
      // Categories without budget_mode that have a parent are handled as nested children.
      val topLevel = categories.filter { it.budgetMode != null || it.parentId == null }

      val grouped = topLevel.groupBy { it.budgetMode }

      val sectionOrder: List<BudgetMode?> =
          listOf(BudgetMode.MONTHLY, BudgetMode.ANNUAL, BudgetMode.PROJECT, null)

      return sectionOrder.mapNotNull { mode ->
        val cats = grouped[mode] ?: return@mapNotNull null
        val sorted = cats.sortedBy { it.name.value.lowercase() }

        val items = buildList {
          for (cat in sorted) {
            val parentName = cat.parentId?.let { byId[it]?.name?.value }
            val displayName = leafName(cat.name, parentName)
            add(
                CategoryDisplayItem(
                    id = cat.id,
                    name = displayName,
                    parentName = parentName,
                    budgetMode = cat.budgetMode,
                    budgetAmount = cat.budgetAmount,
                    transactionCount = cat.transactionCount,
                    isChild = cat.parentId != null,
                )
            )
            // Nest children that don't have their own budget_mode
            for (child in unbudgetedChildrenOf[cat.id].orEmpty()) {
              val childDisplayName = leafName(child.name, cat.name.value)
              add(
                  CategoryDisplayItem(
                      id = child.id,
                      name = childDisplayName,
                      parentName = cat.name.value,
                      budgetMode = null,
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
