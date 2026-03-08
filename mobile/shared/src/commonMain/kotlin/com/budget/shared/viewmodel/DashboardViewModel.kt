package com.budget.shared.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.budget.shared.api.BudgetGroupSummary
import com.budget.shared.api.BudgetMode
import com.budget.shared.api.BudgetMonth
import com.budget.shared.api.BudgetStatus
import com.budget.shared.api.CashFlowSummary
import com.budget.shared.api.PaceIndicator
import com.budget.shared.api.ProjectStatusEntry
import com.budget.shared.api.TransactionEntry
import com.budget.shared.repository.BudgetRepository
import com.budget.shared.repository.DashboardData
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
    val monthlyCashflow: CashFlowSummary? = null,
    val annualCashflow: CashFlowSummary? = null,
    val projectSummary: BudgetSummary = BudgetSummary(),
    val monthlyTransactions: List<TransactionEntry> = emptyList(),
    val annualTransactions: List<TransactionEntry> = emptyList(),
    val projectTransactions: List<TransactionEntry> = emptyList(),
    val selectedCategoryId: String? = null,
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

    val monthly = sortByUrgency(resp.statuses.filter { it.budgetMode == BudgetMode.MONTHLY })
    val annual = sortByUrgency(resp.statuses.filter { it.budgetMode == BudgetMode.ANNUAL })
    val projects = sortProjectsByUrgency(resp.projects)

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

    // Collect all cashflow transactions for the monthly transaction list
    val monthlyCashflowTxns =
        (resp.monthlyCashflow.income.items +
                resp.monthlyCashflow.otherIncome.items +
                resp.monthlyCashflow.unbudgetedSpending.items)
            .flatMap { it.transactions }

    val annualCashflowTxns =
        (resp.annualCashflow.income.items +
                resp.annualCashflow.otherIncome.items +
                resp.annualCashflow.unbudgetedSpending.items)
            .flatMap { it.transactions }

    _uiState.update {
      it.copy(
          loading = false,
          error = null,
          currentMonth = activeMonth,
          months = months,
          monthlyStatuses = monthly,
          annualStatuses = annual,
          projects = projects,
          monthlyCashflow = resp.monthlyCashflow,
          annualCashflow = resp.annualCashflow,
          projectSummary = resp.projectSummary.toUiSummary(),
          monthlyTransactions =
              (resp.monthlyTransactions + monthlyCashflowTxns).sortedByDescending { t ->
                t.postedDate
              },
          annualTransactions =
              (resp.annualTransactions + annualCashflowTxns).sortedByDescending { t ->
                t.postedDate
              },
          projectTransactions = resp.projectTransactions.sortedByDescending { t -> t.postedDate },
          budgetYear = resp.budgetYear,
          monthlyTimeLabel = monthlyTimeLabel,
          annualTimeLabel = annualTimeLabel,
          isCurrentMonth = isCurrentMonth,
          hasPrevMonth = activeMonthIndex > 0,
          hasNextMonth = activeMonthIndex < sortedMonths.size - 1,
      )
    }
  }

  private fun paceOrdinal(pace: PaceIndicator): Int =
      when (pace) {
        PaceIndicator.OVER_BUDGET -> 0
        PaceIndicator.ABOVE_PACE -> 1
        PaceIndicator.ON_TRACK -> 2
        PaceIndicator.UNDER_BUDGET -> 3
        PaceIndicator.PENDING -> 4
      }

  private fun sortByUrgency(items: List<BudgetStatus>): List<BudgetStatus> =
      items.sortedWith(
          compareBy<BudgetStatus> { paceOrdinal(it.pace) }.thenByDescending { it.spent }
      )

  private fun sortProjectsByUrgency(items: List<ProjectStatusEntry>): List<ProjectStatusEntry> =
      items.sortedWith(
          compareBy<ProjectStatusEntry> { paceOrdinal(it.pace) }.thenByDescending { it.spent }
      )
}

private fun BudgetGroupSummary.toUiSummary() =
    BudgetSummary(
        totalBudget = totalBudget,
        totalSpent = totalSpent,
        totalRemaining = remaining,
        overBudgetCount = overBudgetCount,
        barMax = barMax,
    )
