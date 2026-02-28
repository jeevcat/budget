package com.budget.app

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.budget.shared.api.Transaction
import com.budget.shared.viewmodel.DisplayCategory
import com.budget.shared.viewmodel.TransactionsUiState
import com.budget.shared.viewmodel.TransactionsViewModel
import java.text.NumberFormat
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException
import java.util.Currency
import java.util.Locale
import kotlin.math.abs

// -- Formatting helpers (shared with DashboardScreen) ----------------------

private val ShortDateFormatter = DateTimeFormatter.ofPattern("MMM d", Locale.ENGLISH)

private val EuroCurrencyFormat: NumberFormat =
    NumberFormat.getCurrencyInstance(Locale.GERMANY).apply {
      currency = Currency.getInstance("EUR")
    }

private fun formatTransactionAmount(value: String): String {
  val d = value.toDoubleOrNull() ?: return value
  val formatted =
      synchronized(EuroCurrencyFormat) {
        EuroCurrencyFormat.maximumFractionDigits = 0
        EuroCurrencyFormat.format(abs(d))
      }
  val prefix = if (d < 0) "-" else ""
  return "$prefix$formatted"
}

private fun formatTransactionDate(dateStr: String): String =
    try {
      LocalDate.parse(dateStr).format(ShortDateFormatter)
    } catch (_: DateTimeParseException) {
      dateStr
    }

// -- Root screen -----------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TransactionsScreen(viewModel: TransactionsViewModel) {
  val state by viewModel.uiState.collectAsStateWithLifecycle()

  Box(modifier = Modifier.fillMaxSize()) {
    when {
      state.loading && state.transactions.isEmpty() -> {
        CircularProgressIndicator(modifier = Modifier.align(Alignment.Center))
      }
      state.error != null && state.transactions.isEmpty() -> {
        Column(
            modifier = Modifier.align(Alignment.Center).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
          Text(
              text = state.error ?: "Unknown error",
              color = MaterialTheme.colorScheme.error,
          )
          Spacer(modifier = Modifier.height(12.dp))
          TextButton(onClick = viewModel::refresh) { Text("Retry") }
        }
      }
      state.transactions.isEmpty() -> {
        Column(
            modifier = Modifier.align(Alignment.Center).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
          Text(
              text = "All caught up!",
              style = MaterialTheme.typography.headlineSmall,
          )
          Spacer(modifier = Modifier.height(4.dp))
          Text(
              text = "No uncategorized transactions.",
              style = MaterialTheme.typography.bodyMedium,
              color = MaterialTheme.colorScheme.onSurfaceVariant,
          )
        }
      }
      else -> {
        TransactionList(
            state = state,
            onSelect = viewModel::selectTransaction,
        )
      }
    }

    // Category picker bottom sheet
    if (state.selectedTransaction != null) {
      CategoryPickerSheet(
          state = state,
          filteredCategories = viewModel.filteredCategories(),
          onSearchChange = viewModel::updateCategorySearch,
          onCategorySelected = viewModel::categorize,
          onDismiss = { viewModel.selectTransaction(null) },
      )
    }
  }
}

// -- Transaction list ------------------------------------------------------

@Composable
private fun TransactionList(
    state: TransactionsUiState,
    onSelect: (Transaction) -> Unit,
) {
  LazyColumn(
      modifier = Modifier.fillMaxSize(),
      contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 24.dp),
      verticalArrangement = Arrangement.spacedBy(2.dp),
  ) {
    item {
      Text(
          text = "${state.total} uncategorized",
          style = MaterialTheme.typography.labelLarge,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
          modifier = Modifier.padding(bottom = 8.dp),
      )
    }

    if (state.categorizing) {
      item { LinearProgressIndicator(modifier = Modifier.fillMaxWidth()) }
    }

    items(state.transactions, key = { it.id }) { txn ->
      TransactionCard(
          transaction = txn,
          categories = state.categories,
          onClick = { onSelect(txn) },
      )
    }
  }
}

// -- Transaction card ------------------------------------------------------

@Composable
private fun TransactionCard(
    transaction: Transaction,
    categories: List<DisplayCategory>,
    onClick: () -> Unit,
) {
  val merchant = transaction.merchantName.ifEmpty { transaction.description }
  val subtitle = buildString {
    append(formatTransactionDate(transaction.postedDate))
    if (transaction.categoryMethod != null) {
      val catName = categories.find { it.id == transaction.categoryId }?.displayName
      if (catName != null) append(" · $catName")
    }
  }

  ElevatedCard(
      modifier = Modifier.fillMaxWidth(),
  ) {
    Row(
        modifier =
            Modifier.clickable(onClick = onClick)
                .padding(horizontal = 16.dp, vertical = 12.dp)
                .fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
      Column(modifier = Modifier.weight(1f)) {
        Text(
            text = merchant,
            style = MaterialTheme.typography.bodyLarge,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Text(
            text = subtitle,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
      }
      Spacer(modifier = Modifier.width(12.dp))
      Text(
          text = formatTransactionAmount(transaction.amount),
          style = MaterialTheme.typography.bodyLarge,
          fontWeight = FontWeight.Medium,
      )
    }
  }

  // Show LLM suggestion if available
  if (transaction.suggestedCategory != null) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
      Text(
          text = "Suggestion: ${transaction.suggestedCategory}",
          style = MaterialTheme.typography.labelSmall,
          color = MaterialTheme.colorScheme.primary,
      )
    }
  }
}

// -- Category picker bottom sheet ------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CategoryPickerSheet(
    state: TransactionsUiState,
    filteredCategories: List<DisplayCategory>,
    onSearchChange: (String) -> Unit,
    onCategorySelected: (String) -> Unit,
    onDismiss: () -> Unit,
) {
  val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
  val txn = state.selectedTransaction ?: return

  ModalBottomSheet(
      onDismissRequest = onDismiss,
      sheetState = sheetState,
  ) {
    Column(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
    ) {
      // Transaction summary
      Text(
          text = txn.merchantName.ifEmpty { txn.description },
          style = MaterialTheme.typography.titleMedium,
          fontWeight = FontWeight.Bold,
          maxLines = 1,
          overflow = TextOverflow.Ellipsis,
      )
      Row(
          modifier = Modifier.fillMaxWidth(),
          horizontalArrangement = Arrangement.SpaceBetween,
      ) {
        Text(
            text = formatTransactionDate(txn.postedDate),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = formatTransactionAmount(txn.amount),
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
        )
      }

      // Extra details
      val counterpartyName = txn.counterpartyName
      if (counterpartyName != null) {
        Text(
            text = counterpartyName,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
      }
      if (txn.description.isNotEmpty() && txn.merchantName.isNotEmpty()) {
        Text(
            text = txn.description,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
      }

      // LLM suggestion
      if (txn.suggestedCategory != null) {
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = "AI suggestion: ${txn.suggestedCategory}",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.primary,
        )
        val llmJustification = txn.llmJustification
        if (llmJustification != null) {
          Text(
              text = llmJustification,
              style = MaterialTheme.typography.bodySmall,
              color = MaterialTheme.colorScheme.onSurfaceVariant,
              maxLines = 2,
              overflow = TextOverflow.Ellipsis,
          )
        }
      }

      Spacer(modifier = Modifier.height(12.dp))
      HorizontalDivider()
      Spacer(modifier = Modifier.height(8.dp))

      // Search field
      OutlinedTextField(
          value = state.categorySearch,
          onValueChange = onSearchChange,
          modifier = Modifier.fillMaxWidth(),
          placeholder = { Text("Search categories") },
          leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
          singleLine = true,
      )

      Spacer(modifier = Modifier.height(8.dp))

      // Category list
      if (state.categorizing) {
        LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
        Spacer(modifier = Modifier.height(8.dp))
      }

      LazyColumn(
          modifier = Modifier.fillMaxWidth().weight(1f, fill = false),
      ) {
        items(filteredCategories, key = { it.id }) { category ->
          val isCurrentCategory = txn.categoryId == category.id
          ListItem(
              headlineContent = {
                Text(
                    text = category.name,
                    fontWeight = if (category.depth == 0) FontWeight.Medium else FontWeight.Normal,
                )
              },
              supportingContent = category.parentName?.let { name -> { Text(name) } },
              leadingContent =
                  if (category.depth > 0) {
                    { Spacer(modifier = Modifier.width(24.dp)) }
                  } else null,
              trailingContent =
                  if (isCurrentCategory) {
                    { Icon(Icons.Default.Check, contentDescription = "Current") }
                  } else null,
              modifier =
                  Modifier.clickable(enabled = !state.categorizing) {
                    onCategorySelected(category.id)
                  },
          )
        }
      }

      // Bottom padding for nav bar
      Spacer(modifier = Modifier.height(16.dp))
    }
  }
}
