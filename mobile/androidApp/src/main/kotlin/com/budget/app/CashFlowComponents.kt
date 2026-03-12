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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
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
import com.budget.shared.api.CashFlowItem
import com.budget.shared.api.CashFlowSummary

private val PositiveColor = Color(0xFF76946A)
private val NegativeColor = Color(0xFFC34043)

@Composable
private fun SummaryCell(
    label: String,
    value: String,
    valueColor: Color,
    modifier: Modifier = Modifier,
) {
  Surface(
      modifier = modifier,
      shape = RoundedCornerShape(8.dp),
      color = MaterialTheme.colorScheme.surfaceVariant,
  ) {
    Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
      Text(
          text = label,
          style = MaterialTheme.typography.labelSmall,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
      )
      Text(
          text = value,
          style = MaterialTheme.typography.titleMedium,
          fontWeight = FontWeight.Bold,
          color = valueColor,
      )
    }
  }
}

@Composable
private fun CashFlowSummaryStrip(cashflow: CashFlowSummary, hasIncome: Boolean) {
  Column(
      modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 12.dp),
      verticalArrangement = Arrangement.spacedBy(8.dp),
  ) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
      SummaryCell(
          label = "Net",
          value = formatAmount(cashflow.netCashflow, showSign = true),
          valueColor = if (cashflow.netCashflow < 0) NegativeColor else PositiveColor,
          modifier = Modifier.weight(1f),
      )
      if (hasIncome) {
        SummaryCell(
            label = "Saved",
            value = formatAmount(cashflow.saved, showSign = true),
            valueColor = if (cashflow.saved < 0) NegativeColor else PositiveColor,
            modifier = Modifier.weight(1f),
        )
      }
    }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
      SummaryCell(
          label = "Total In",
          value = formatAmount(cashflow.totalIn, showSign = true),
          valueColor = PositiveColor,
          modifier = Modifier.weight(1f),
      )
      SummaryCell(
          label = "Total Out",
          value = formatAmount(cashflow.totalOut),
          valueColor = NegativeColor,
          modifier = Modifier.weight(1f),
      )
    }
  }
}

@Composable
private fun CashFlowItemCard(item: CashFlowItem, onItemClick: (CashFlowItem) -> Unit) {
  Surface(
      shape = RoundedCornerShape(8.dp),
      tonalElevation = 1.dp,
      modifier = Modifier.fillMaxWidth().clickable { onItemClick(item) },
  ) {
    Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
      Row(
          modifier = Modifier.fillMaxWidth(),
          horizontalArrangement = Arrangement.SpaceBetween,
          verticalAlignment = Alignment.CenterVertically,
      ) {
        Text(
            text = item.label,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
        )
        Text(
            text = formatAmount(item.amount),
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Bold,
        )
      }
      Text(
          text = "${item.transactionCount} transactions",
          style = MaterialTheme.typography.labelSmall,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
      )
    }
  }
}

@Composable
private fun CashFlowPlainRow(label: String, amount: String) {
  Row(
      modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
      horizontalArrangement = Arrangement.SpaceBetween,
      verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(
        text = label,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    Text(
        text = amount,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
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
internal fun CashFlowCard(
    cashflow: CashFlowSummary,
    onItemClick: (CashFlowItem) -> Unit,
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

      CashFlowSummaryStrip(cashflow = cashflow, hasIncome = hasIncome)

      AnimatedVisibility(visible = expanded) {
        Column(
            modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 16.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
          if (hasIncome || hasOtherIncome) {
            SectionLabel("IN")
            cashflow.income.items.forEach { item -> CashFlowItemCard(item, onItemClick) }
            cashflow.otherIncome.items.forEach { item -> CashFlowItemCard(item, onItemClick) }
          }

          Spacer(modifier = Modifier.height(4.dp))

          SectionLabel("OUT")
          if (cashflow.budgetedSpendingTotal > 0) {
            CashFlowPlainRow("Budgeted Spending", formatAmount(cashflow.budgetedSpendingTotal))
          }
          cashflow.unbudgetedSpending.items.forEach { item -> CashFlowItemCard(item, onItemClick) }
        }
      }
    }
  }
}
