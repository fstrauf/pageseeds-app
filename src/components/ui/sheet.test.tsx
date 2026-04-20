import { describe, it } from 'vitest'
import { render } from '@testing-library/react'
import { Sheet, SheetContent } from './sheet'

describe('Sheet', () => {
  it('renders without crashing', () => {
    render(
      <Sheet open={false}>
        <SheetContent>
          <div>Content</div>
        </SheetContent>
      </Sheet>
    )
  })
})
