import { describe, it } from 'vitest'
import { render } from '@testing-library/react'
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from './select'
import { ToastProvider } from '../../lib/toast-context'

describe('Select', () => {
  it('renders without crashing', () => {
    render(
      <ToastProvider>
        <Select value="all">
          <SelectTrigger className="w-32 h-8 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All</SelectItem>
            <SelectItem value="todo">To-do</SelectItem>
          </SelectContent>
        </Select>
      </ToastProvider>
    )
  })
})
