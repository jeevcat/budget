package com.budget.shared.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.budget.shared.api.BudgetMode
import com.budget.shared.api.BudgetMonth
import com.budget.shared.api.BudgetStatus
import com.budget.shared.api.PaceIndicator
import com.budget.shared.api.ProjectStatusEntry
import com.budget.shared.api.Summarizable
import com.budget.shared.api.TransactionEntry
import com.budget.shared.repository.BudgetRepository
import com.budget.shared.repository.DashboardData
import kotlin.math.abs
import kotlin.math.max
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class BudgetSummary(
    val totalBudget: Double = 0.0,
    val totalSpent: Double = 0.0,
    val totalRemaining: Double = 0.0,
    val overBudgetCount: Int = 0,
    val barMax: Double = 1.0,
)

data class DashboardUiState(
    val loading: Boolean = true,
    val error: String? = null,
    val currentMonth: BudgetMonth? = null,
    val months: List<BudgetMonth> = emptyList(),
    val selectedTab: BudgetMode = BudgetMode.MONTHLY,
    val monthlyStatuses: List<BudgetStatus> = emptyList(),
    val annualStatuses: List<BudgetStatus> = emptyList(),
    val projects: List<ProjectStatusEntry> = emptyList(),
    val monthlySummary: BudgetSummary = BudgetSummary(),
    val annualSummary: BudgetSummary = BudgetSummary(),
    val projectSummary: BudgetSummary = BudgetSummary(),
    val monthlyTransactions: List<TransactionEntry> = emptyList(),
    val annualTransactions: List<TransactionEntry> = emptyList(),
    val projectTransactions: List<TransactionEntry> = emptyList(),
    val selectedCategoryId: String? = null,
    val unbudgetedSpent: Double = 0.0,
    val unbudgetedTransactions: List<TransactionEntry> = emptyList(),
    val uncategorizedCount: Int = 0,
    val budgetYear: Int = 0,
    val monthlyTimeLabel: String = "",
    val annualTimeLabel: String = "",
    val isCurrentMonth: Boolean = false,
    val hasPrevMonth: Boolean = false,
    val hasNextMonth: Boolean = false,
)

class DashboardViewModel(
    private val repository: BudgetRepository,
) : ViewModel() {

  private val _uiState = MutableStateFlow(DashboardUiState())
  val uiState: StateFlow<DashboardUiState> = _uiState.asStateFlow()

  private var sortedMonths: List<BudgetMonth> = emptyList()
  private var activeMonthIndex: Int = -1
  private val cache = mutableMapOf<String, DashboardData>()

  init {
    load(monthId = null)
    viewModelScope.launch {
      repository.invalidationEvents.collect {
        cache.clear()
        val currentMonthId = _uiState.value.currentMonth?.id
        load(monthId = currentMonthId)
      }
    }
  }

  fun selectTab(tab: BudgetMode) {
    _uiState.update { it.copy(selectedTab = tab, selectedCategoryId = null) }
  }

  fun selectCategory(categoryId: String?) {
    _uiState.update { state ->
      val newId = if (state.selectedCategoryId == categoryId) null else categoryId
      state.copy(selectedCategoryId = newId)
    }
  }

  fun goToPreviousMonth() {
    if (activeMonthIndex <= 0) return
    val prev = sortedMonths[activeMonthIndex - 1]
    load(monthId = prev.id)
  }

  fun goToNextMonth() {
    if (activeMonthIndex >= sortedMonths.size - 1) return
    val next = sortedMonths[activeMonthIndex + 1]
    load(monthId = next.id)
  }

  private fun load(monthId: String?) {
    // Clear category selection when switching months
    _uiState.update { it.copy(selectedCategoryId = null) }

    // Serve from cache instantly when available
    if (monthId != null) {
      val cached = cache[monthId]
      if (cached != null) {
        processResult(cached)
        return
      }
    }

    _uiState.update { it.copy(loading = true, error = null) }
    viewModelScope.launch {
      try {
        val result = repository.getDashboardData(monthId)
        cache[result.status.month.id] = result
        processResult(result)
        prefetchAdjacentMonths()
      } catch (e: Exception) {
        _uiState.update { it.copy(loading = false, error = e.message ?: "Unknown error") }
      }
    }
  }

  private fun prefetchAdjacentMonths() {
    if (activeMonthIndex > 0) {
      val prevId = sortedMonths[activeMonthIndex - 1].id
      if (prevId !in cache) {
        viewModelScope.launch {
          try {
            val result = repository.getDashboardData(prevId)
            cache[result.status.month.id] = result
          } catch (_: Exception) {
            /* silent prefetch failure */
          }
        }
      }
    }
    if (activeMonthIndex < sortedMonths.size - 1) {
      val nextId = sortedMonths[activeMonthIndex + 1].id
      if (nextId !in cache) {
        viewModelScope.launch {
          try {
            val result = repository.getDashboardData(nextId)
            cache[result.status.month.id] = result
          } catch (_: Exception) {
            /* silent prefetch failure */
          }
        }
      }
    }
  }

  private fun processResult(result: DashboardData) {
    val resp = result.status
    val months = result.months

    sortedMonths = months.sortedBy { it.startDate }
    val activeMonth = resp.month
    activeMonthIndex = sortedMonths.indexOfFirst { it.id == activeMonth.id }
    val isCurrentMonth = activeMonth.endDate == null

    val monthly =
        resp.statuses.filter { it.budgetMode == BudgetMode.MONTHLY }.sortedByDescending { it.spent }
    val annual =
        resp.statuses.filter { it.budgetMode == BudgetMode.ANNUAL }.sortedByDescending { it.spent }
    val projects = resp.projects.sortedByDescending { it.spent }

    val monthlyTimeLabel =
        if (monthly.isNotEmpty()) {
          val tl = monthly.first().timeLeft
          if (tl == null) "open-ended" else "${tl}d left"
        } else ""

    val annualTimeLabel =
        if (annual.isNotEmpty()) {
          val tl = annual.first().timeLeft
          if (tl == null) "open-ended" else "${tl}mo left"
        } else ""

    _uiState.update {
      it.copy(
          loading = false,
          error = null,
          currentMonth = activeMonth,
          months = months,
          monthlyStatuses = monthly,
          annualStatuses = annual,
          projects = projects,
          monthlySummary = computeSummary(monthly),
          annualSummary = computeSummary(annual),
          projectSummary = computeSummary(projects),
          monthlyTransactions =
              (resp.monthlyTransactions + resp.unbudgetedTransactions).sortedByDescending { t ->
                t.postedDate
              },
          unbudgetedSpent = resp.unbudgetedSpent,
          unbudgetedTransactions =
              resp.unbudgetedTransactions.sortedByDescending { t -> t.postedDate },
          annualTransactions = resp.annualTransactions.sortedByDescending { t -> t.postedDate },
          projectTransactions = resp.projectTransactions.sortedByDescending { t -> t.postedDate },
          uncategorizedCount = resp.uncategorizedCount,
          budgetYear = resp.budgetYear,
          monthlyTimeLabel = monthlyTimeLabel,
          annualTimeLabel = annualTimeLabel,
          isCurrentMonth = isCurrentMonth,
          hasPrevMonth = activeMonthIndex > 0,
          hasNextMonth = activeMonthIndex < sortedMonths.size - 1,
      )
    }
  }

  private fun computeSummary(items: List<Summarizable>): BudgetSummary {
    val budget = items.sumOf { it.budgetAmount }
    val spent = items.sumOf { it.spent }
    val overCount = items.count { it.pace == PaceIndicator.OVER_BUDGET }
    val barMax = items.maxOfOrNull { max(abs(it.spent), it.budgetAmount) } ?: 1.0
    return BudgetSummary(budget, spent, budget - spent, overCount, barMax)
  }
}
