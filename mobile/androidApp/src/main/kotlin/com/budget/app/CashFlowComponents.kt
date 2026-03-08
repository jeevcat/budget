package com.budget.app

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.budget.shared.api.BudgetMode
import com.budget.shared.api.CashFlowItem
import com.budget.shared.api.CashFlowSummary
import com.budget.shared.viewmodel.DashboardUiState

private val PositiveColor = Color(0xFF76946A)
private val NegativeColor = Color(0xFFC34043)

internal fun activeCashflow(state: DashboardUiState) =
    when (state.selectedTab) {
      BudgetMode.MONTHLY -> state.monthlyCashflow
      BudgetMode.ANNUAL -> state.annualCashflow
      else -> null
    }

internal fun findCashFlowItem(cashflow: CashFlowSummary?, categoryId: String) =
    cashflow?.let {
      (it.income.items + it.otherIncome.items + it.unbudgetedSpending.items).find { item ->
        item.categoryId == categoryId
      }
    }

@Composable
internal fun CashFlowCard(
    cashflow: CashFlowSummary,
    onCategoryClick: (String) -> Unit,
    startExpanded: Boolean = true,
) {
  val hasIncome = cashflow.income.total > 0
  val hasOtherIncome = cashflow.otherIncome.items.isNotEmpty()
  val hasUnbudgeted = cashflow.unbudgetedSpending.items.isNotEmpty()
  if (!(hasIncome || hasOtherIncome || hasUnbudgeted)) return

  var expanded by remember { mutableStateOf(startExpanded) }

  ElevatedCard(modifier = Modifier.fillMaxWidth()) {
    Column {
      Row(
          modifier = Modifier.fillMaxWidth().clickable { expanded = !expanded }.padding(16.dp),
          horizontalArrangement = Arrangement.SpaceBetween,
          verticalAlignment = Alignment.CenterVertically,
      ) {
        Text(
            text = "Cash Flow",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
        )
        Icon(
            imageVector = if (expanded) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore,
            contentDescription = if (expanded) "Collapse" else "Expand",
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
      }
      AnimatedVisibility(visible = expanded) {
        Column(modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 16.dp)) {
          CashFlowInSection(cashflow, onCategoryClick)
          CashFlowOutSection(cashflow, onCategoryClick)
          CashFlowNetSection(cashflow, hasIncome)
        }
      }
    }
  }
}

@Composable
private fun CashFlowInSection(cashflow: CashFlowSummary, onCategoryClick: (String) -> Unit) {
  val hasIncome = cashflow.income.total > 0
  val hasOtherIncome = cashflow.otherIncome.items.isNotEmpty()
  if (!(hasIncome || hasOtherIncome)) return

  SectionLabel("IN")
  CashFlowItemRows(cashflow.income.items, onCategoryClick)
  if (hasOtherIncome) {
    CashFlowItemRows(cashflow.otherIncome.items, onCategoryClick)
  }
  CashFlowTotalRow(
      label = "Total In",
      amount = formatAmount(cashflow.totalIn, showSign = true),
      color = PositiveColor,
  )
  Spacer(modifier = Modifier.height(12.dp))
}

@Composable
private fun CashFlowOutSection(cashflow: CashFlowSummary, onCategoryClick: (String) -> Unit) {
  SectionLabel("OUT")
  if (cashflow.budgetedSpending.total > 0) {
    CashFlowRow(label = "Budgeted Spending", amount = formatAmount(cashflow.budgetedSpending.total))
  }
  if (cashflow.unbudgetedSpending.items.isNotEmpty()) {
    CashFlowItemRows(cashflow.unbudgetedSpending.items, onCategoryClick)
  }
  CashFlowTotalRow(
      label = "Total Out",
      amount = formatAmount(cashflow.totalOut),
      color = NegativeColor,
  )
  Spacer(modifier = Modifier.height(12.dp))
}

@Composable
private fun CashFlowNetSection(cashflow: CashFlowSummary, hasIncome: Boolean) {
  CashFlowTotalRow(
      label = "Net",
      amount = formatAmount(cashflow.netCashflow, showSign = true),
      color = if (cashflow.netCashflow < 0) NegativeColor else PositiveColor,
      bold = true,
  )
  if (hasIncome) {
    Spacer(modifier = Modifier.height(2.dp))
    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
      Text(
          text = "Saved from salary",
          style = MaterialTheme.typography.bodySmall,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
      )
      Text(
          text = formatAmount(cashflow.saved, showSign = true),
          style = MaterialTheme.typography.bodySmall,
          fontWeight = FontWeight.Medium,
          color = if (cashflow.saved < 0) NegativeColor else PositiveColor,
      )
    }
  }
}

@Composable
private fun SectionLabel(text: String) {
  Text(
      text = text,
      style = MaterialTheme.typography.labelMedium,
      color = MaterialTheme.colorScheme.onSurfaceVariant,
  )
  Spacer(modifier = Modifier.height(4.dp))
}

@Composable
private fun CashFlowItemRows(items: List<CashFlowItem>, onCategoryClick: (String) -> Unit) {
  items.forEach { item ->
    CashFlowRow(
        label = item.label,
        amount = formatAmount(item.amount),
        onClick = item.categoryId?.let { id -> { onCategoryClick(id) } },
    )
  }
}

@Composable
private fun CashFlowRow(label: String, amount: String, onClick: (() -> Unit)? = null) {
  Row(
      modifier =
          Modifier.fillMaxWidth()
              .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
              .padding(vertical = 2.dp),
      horizontalArrangement = Arrangement.SpaceBetween,
  ) {
    Text(text = label, style = MaterialTheme.typography.bodyMedium)
    Text(text = amount, style = MaterialTheme.typography.bodyMedium)
  }
}

@Composable
private fun CashFlowTotalRow(label: String, amount: String, color: Color, bold: Boolean = false) {
  Row(
      modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
      horizontalArrangement = Arrangement.SpaceBetween,
  ) {
    Text(
        text = label,
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = if (bold) FontWeight.Bold else FontWeight.SemiBold,
    )
    Text(
        text = amount,
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = if (bold) FontWeight.Bold else FontWeight.SemiBold,
        color = color,
    )
  }
}
